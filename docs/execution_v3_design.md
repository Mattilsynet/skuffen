# Execution v3 — operasjonsbasert eksekvering

Status: **implementert.** Dette dokumentet er beslutningsgrunnlaget for omskrivingen av
eksekveringssystemet, og er nå den gjeldende beskrivelsen. Det erstatter
`docs/execution_v2_design.md` og `docs/command_executor.md`, som er slettet.
Normativ ADR: [SKU-0016](adr/skuffen/SKU-0016-operasjonsbasert-eksekvering.md).

Avvik fra planen, besluttet under implementasjonen:

- **`muterer_arkivet(RenderDokument) = false`.** D7 sa `AvventJournalført` var eneste `false`, men
  rendring skriver til object store på en deterministisk UUIDv5-nøkkel og er idempotent. Å sende den
  gjennom `sendt`-fasen ville gjort en selvhelbredende crash til manuell opprydding.
- **`DokumentTilstand` skiller innhold fra arkiv.** Prerequisitene trenger to fakta: «innholdet er
  klart» (leses av `OpprettJournalpost`) og «dokumentet ligger i arkivet» (leses av `Journalfør`).
  Én `Ok`-verdi kan ikke bære begge, og gjorde prerequisiten sirkulær. Kjeden er
  `AvventerRendring → Klar → Ok`.
- **`kommando.payload` finnes ikke.** D26 begrunnet den med kvittering og re-dekomponering. Begge er
  dekket: `dekomponer` trenger bare `kommandotype` pluss det som allerede er materialisert i state,
  og klientens innsending ligger i `arkiv_command_inbox` i 180 dager.
- **Transiente valideringsutfall publiserer ingenting.** `blokkert` og `recoverable` gir ikke event;
  kommandoen redeliveres av NATS. Følger D33 — vi publiserer utfall, ikke flakking.
- **Statuskontekst joines fra state** ved publisering i stedet for å materialiseres på kommandoen.
  Saksnummeret oppstår først når `OpprettSak` returnerer, så en materialisert kontekst ville vært
  tom nettopp når den betyr noe.

Dokumentet er skrevet for å være selvbærende: en agent eller utvikler skal kunne starte herfra uten
å lese seg gjennom dagens execution-kode først.

## Innhold

- [Hvorfor](#hvorfor)
- [Kjernemodell](#kjernemodell)
- [Beslutninger](#beslutninger)
- [Operasjonslister](#operasjonslister)
- [Domenefunksjoner](#domenefunksjoner)
- [Databaseskjema](#databaseskjema)
- [NATS-kontrakt](#nats-kontrakt)
- [Slettelisten](#slettelisten)
- [Kjente defekter som fikses](#kjente-defekter-som-fikses)
- [Faseplan](#faseplan)
- [Åpne punkter](#åpne-punkter)

---

## Hvorfor

Dagens system (execution v2, SKU-0001/SKU-0007) er **kommandobasert**. Én rad i `command_execution`
per kommando, og den rene domenefunksjonen `planlegg_neste_handling(command, facts)` beregner ved
hvert forsøk hvilken *ene* `ArkivOperasjon` som skal utføres nå. Operasjoner er dermed flyktige —
de finnes bare som en verdi i minnet under ett forsøk.

Det gir fire konkrete problemer:

1. **Ingen operasjon har identitet.** En kommando som oppretter en journalpost med fem vedlegg gjør
   åtte arkivkall, men systemet kan ikke navngi, spore, retrye eller rapportere status for noen av
   dem enkeltvis.
2. **Ingen at-most-once-grense.** Sikri har ingen idempotency key (verifisert mot swagger:
   `ElementsSak` og `ElementsJournalpost` har ingen ekstern nøkkel), og det finnes ingen rad å
   journalføre intensjonen i før arkivkallet. En DB-feil etter et vellykket arkivskriv gir duplikat
   i arkivet.
3. **Feilisolasjon mangler.** Ett feilende vedlegg stopper hele kommandoen, i strid med det
   dokumenterte best-effort-prinsippet (`.agent/guides/architecture/flows/Skuffen - Partial success,
   best effort.svg`).
4. **Re-planlegging ved hvert forsøk** gjør at «hva gjenstår» aldri er lesbart fra databasen.

Modellen vi går til er allerede tegnet i
`.agent/guides/architecture/command/Skuffen - Kommando mapping til Operasjon.svg`: kommando →
«lag jobber» → flat liste av operasjoner.

Kjerneregelen fra v2 beholdes: **sak state + operasjon + domeneregler ⇒ hva gjør vi.** Forskjellen
er at operasjonen nå er en persistert rad med egen id, ikke en beregnet verdi.

**Uendret scope:** ingestion, validering, eksterne kontrakter, wire-typer og DTO-er. Omskrivingen
treffer eksekveringssystemet og de tilhørende databasetabellene.

**Greenfield.** Det finnes ingen reelle klienter og ingen produksjonsdata. Migrasjonshistorikken
nullstilles; databasen slettes og bygges på nytt. Ingen forward-only kompatibilitetsmigrasjon.

---

## Kjernemodell

En **operasjon** er ett kall mot arkiv-API-et. Den har egen id, egen livssyklus, egen retry og egen
status. Én kommando har én eller flere operasjoner; én operasjon tilhører nøyaktig én kommando.

Tre lag med tydelig eierskap:

| Lag | Tabell | Eier |
|---|---|---|
| Identitet | `entitet` | skuffen_id ↔ client_reference ↔ arkiv_id |
| Fakta | `sak_tilstand`, `journalpost_tilstand`, `dokument_tilstand` | hva som er sant nå |
| Eksekvering | `kommando`, `operasjon`, `operasjon_forsok` | hva vi har prøvd og hva som gjenstår |

Testen som avgjør hvor noe hører hjemme: **hvis du sletter alle operasjonsrader, må systemet
fortsatt kunne svare på «hva er sant om denne saken?».** Fakta kan derfor aldri bo på en operasjon.

---

## Beslutninger

Normative. Nummerert for referanse fra ADR og kode.

### Modell og planlegging

**D1.** En operasjon er ett arkivkall, med egen id (`Uuid::now_v7`), egen status, egen retry og egen
statuslinje utad.

**D2.** Dekomponering fra kommando til operasjoner skjer **én gang**, når den validerte kommandoen
leses inn i eksekveringssystemet. Det finnes ingen re-planlegging. Operasjonslisten er en ren
funksjon av command payload — type, `med_utsending` og antall dokumenter avgjør den fullt ut, uten
avhengighet til facts.

**D3.** Avhengigheter mellom operasjoner **utledes fra facts**, ikke fra lagrede kanter. Ingen
`depends_on`-kolonne. En operasjon blir kjørbar når prerequisites er oppfylt; søskenoperasjoner uten
avhengighet til hverandre kan kjøre i vilkårlig rekkefølge. Ingen `sekvensnr`.

**D4.** `AvsluttSak` er det eneste unntaket fra facts-only-regelen. Dens prerequisite er at **alle
andre operasjoner på saken er terminalt ok** — ikke bare at journalpostene er ferdige. Uten dette
ville regelen ikke fanget f.eks. `SettSaksansvarlig`. Den får derfor en egen domenefunksjon.

**D5.** Én operasjon per vedlegg. Vedlegg kan være store, og partial success fra Sikris
`LeggTilVedleggPaaJournalpost` (som returnerer `Vec<Option<i32>>`) er ikke håndterbart i batch.

**D6.** `RenderDokument` er en egen operasjon, selv om den ikke er et arkivkall.

**D7.** Ingen `art`-taksonomi for operasjoner. Behovet dekkes av ett domenepredikat,
`muterer_arkivet(operasjonstype) -> bool`, som styrer om operasjonen går gjennom `sendt`-fasen.
`AvventJournalført` er eneste `false` i dag.

### Utførelse

**D8.** Skrive-operasjoner kjøres i to faser:

1. commit `klar → sendt` **før** HTTP-kallet
2. HTTP-kallet
3. commit `sendt → ok`, arkivsvar og faktaoppdatering i **én** transaksjon

Dette er stedet at-most-once-grensen registreres.

**D9.** En operasjon funnet i `sendt` ved recovery har ukjent utfall og går til `krever_avklaring`.
**Ingen automatisk rekonsiliering.** Et menneske rydder opp. Automatisk rekonsiliering mot
`HentArkivsak`/`HentJournalpost` er vurdert og forkastet: mye kompleksitet, liten gevinst, ny
feilkilde. Idempotency key fra Sikri kommer i en senere versjon.

**D10.** Recoverable feil retryes **for alltid**, med eksponentiell backoff opp til én gang per døgn
(1m, 5m, 15m, 1h, 6h, 12h, 24h, 24h …). Ingen maks antall forsøk. Kun irrecoverable feil gir
terminal `feilet`.

**D11.** Enhver operasjon som ikke er terminal innen 24 timer emitter en **advisory** `varsel` på
statusstrømmen og fortsetter å prøve. Varselet er ikke terminalt og avbryter ingenting. Én uniform
systemregel — ingen per-type-tidsfrister.

**D12.** Ett permanent feilet vedlegg stopper ikke søskenoperasjoner. Best effort: alt som lovlig
kan utføres, utføres. `Journalfør` vil da forbli blokkert, og `AvsluttSak` likeså, til et menneske
rydder.

### Terminal

**D13.** En operasjon er terminal `ok` eller terminal `feilet`.

**D14.** En kommando er terminal `ok` når **alle** operasjoner er terminalt ok. En kommando er
terminal `feilet` når **minst én** operasjon er terminalt feilet.

**D15.** Terminal feil publiseres **umiddelbart**, ikke ved quiescence. Foldet er monotont — når én
operasjon har feilet terminalt kan resultatet aldri gå tilbake til ok — så eventet er sant i det
øyeblikket det sendes og kan aldri trekkes tilbake. Terminal ok kommer naturlig ved quiescence,
siden den krever universalitet.

**D16.** Kontrakten sier: `terminal: true` betyr **«utfallet er avgjort»**, ikke «ingen flere
eventer». Operasjonseventer kan fortsette å komme etterpå, fordi søsken kjører videre best effort.
Dette skal stå eksplisitt i wire-dokumentasjonen.

### Arkivfag

**D17.** Journalposter opprettes aldri direkte i `J`. `Journalfør`, `SettEkspedert`,
`KlargjørForEkspedering` og `Avskriv` er **eksplisitte operasjoner**. At inngående og internt notat i
dag opprettes med `journalstatus="J"` og `avskrivDirekte=true` var en bug: en journalført journalpost
er låst, så vedlegg kunne ikke legges til etterpå.

**D18.** Ved opprettelse settes `journalstatus` ikke i det hele tatt for I og X. Bekreftet at Sikri
åpner journalposten i en status der endringer er mulige. `avskrivDirekte` og `avskrivningsmaate`
settes ikke ved opprettelse.

**D19.** Skuffen setter **aldri `J` på utgående**. Arkivfaglig statusløp:

| Variant | Løp |
|---|---|
| Utgående med utsending | `R` → **`F`** *(Skuffen)* → `E` *(SvarUt-robot, 1–2 min)* → `J` *(RPA-robot, 0,5–1 t)* |
| Utgående uten utsending | `R` → **`E`** *(Skuffen)* → `J` *(RPA-robot)* |

**D20.** Begge utgående varianter får `AvventJournalført`: RPA journalfører i begge løp, så uten
observasjon vet ikke Skuffen at journalposten er ferdig. Polling av `HentJournalpost` hver time,
terminal ved `J`, med observert `E` skrevet som fakta underveis.

**D21.** Utgående journalposter avskrives aldri. Kun inngående avskrives (`TE`).

**D22.** `Journalfør`, `SettEkspedert` og `KlargjørForEkspedering` er tre operasjonstyper selv om
alle er `PUT SetJournalpostStatus`. Regelen er «én operasjon = ett API-kall», ikke «én operasjonstype
= ett endepunkt». Prerequisites og betydning er forskjellige, og statusstrømmen skal være lesbar.

### Data og identitet

**D23.** `id_mapping` blir `entitet` — identitetstabellen og master for `skuffen_id`. Den består
fordi skuffen_id mintes ved ingest, før vi vet om entiteten noensinne får en state-rad: en kommando
kan mottas, id-er deles ut, og så feile validering. Det livsløpsgapet kan state-tabellene ikke dekke.

**D24.** `command_id` flyttes ut av identitetstabellen til `kommando`. Idempotency-nøkkelen er
`kommando.dispatchet_at`, ikke radens eksistens: raden skrives ved mottak, `dispatchet_at` settes
etter vellykket dispatch. Fordi nøkkelen er `command_id` og ikke `client_reference`, blir
`AvsluttSak` og `SettSaksansvarlig` idempotente for første gang.

**D25.** Arkiv-id-er bor i `entitet.arkiv_id` — ett sted. `sak_tilstand.saksnummer`,
`sak_tilstand.sikri_id`, `journalpost_tilstand.sikri_id` og `journalpost_tilstand.journalpostnummer`
fjernes. De to siste settes i dag til samme verdi.

**D26.** Dekomponering **materialiserer attributter inn i state**. Executor leser aldri
`kommando.payload` og rører aldri wire-typer. `payload` er kvittering og grunnlag for
re-dekomponering, ikke eksekveringsinput. Dette fjerner den posisjonelle koblingen mellom id-liste og
payload-liste som finnes i dag.

**D27.** Hoveddokument gjøres eksplisitt i `dokument_tilstand` med `rekkefolge` (= array-indeks i
DTO-en, som beholder «første i lista er hoveddokument») og `er_hoveddokument`, låst sammen med
`CHECK (er_hoveddokument = (rekkefolge = 0))`. I dag er dette ikke persistert noe sted og overlever
kun fordi `Uuid::now_v7()` genereres i payload-rekkefølge i samme prosess.

**D28.** `operasjon.entitet_id` får en **svak FK** mot `entitet(skuffen_id)`. Databasen garanterer
at entiteten finnes, ikke at den er av riktig type for operasjonstypen. Domeneregler lever i
domenekoden, ikke i skjemaet.

**D29.** `operasjon.sak_id` er en denormalisert **partisjonsnøkkel**, ikke identitet. Den finnes
fordi `entitet_id` er polymorf: uten den krever evalueringspasset og `AvsluttSak`-regelen en firveis
`LEFT JOIN` med `COALESCE` over de tre state-tabellene. Med den er begge én indeksskann.

**D30.** `tilstand_historikk` slettes. `operasjon` + `operasjon_forsok` forteller samme historie med
bedre struktur.

### Status utad

**D31.** Én statusstrøm. Stream `arkiv_status`, subjects `arkiv.status.>`. Ingen egen operasjon- eller
loggstrøm — strømmen **er** loggen, og en klient som vil ha historikken lager en consumer med
`DeliverPolicy::All`.

**D32.** Kommandostatus forenkles til én flat hendelse:
`mottatt | validert | avvist | utfores | fullfort | feilet`. Dagens 3×5-matrise av `phase` ×
`status`, med `unreachable!()` for ulovlige kombinasjoner, utgår.

**D33.** `Nats-Msg-Id` bruker id-er vi allerede har i databasen:
`<command_id>:<hendelse>` og `<operasjon_id>:<attempt_no>`. Operasjonsstatus publiseres kun ved
forsøksutfall, ikke ved `blokkert ↔ klar`-flakking; blokkeringsårsak er spørrbar tilstand, ikke en
hendelse.

**D34.** `arkiv.command.done` og hele `arkiv_command_done`-strømmen slettes. Ingenting konsumerer
den, og statusstrømmen bærer terminal.

**D35.** Ingen sak-visning i denne omgang, hverken som subscription eller query. Sak-`client_reference`
finnes kun når klienten selv opprettet saken, og saksnummer inneholder `/` som ikke er lovlig i et
NATS-subject.

---

## Operasjonslister

Ren funksjon av command payload. `RenderDokument?` betyr «kun når hoveddokumentet er en
HTML-template».

| Kommando | Operasjoner |
|---|---|
| `OpprettSak` | `OpprettSak` |
| `OpprettInngående` | `RenderDokument?`, `OpprettJournalpost(I)`, `LeggTilVedlegg×n`, `Journalfør(J)`, `Avskriv(TE)` |
| `OpprettUtgående uten utsending` | `RenderDokument?`, `OpprettJournalpost(U, R)`, `LeggTilVedlegg×n`, `SettEkspedert(E)`, `AvventJournalført` |
| `OpprettUtgående med utsending` | `RenderDokument?`, `OpprettJournalpost(U, R)`, `LeggTilVedlegg×n`, `KlargjørForEkspedering(F)`, `AvventJournalført` |
| `OpprettInterntNotat` | `RenderDokument?`, `OpprettJournalpost(X)`, `LeggTilVedlegg×n`, `Journalfør(J)` |
| `AvsluttSak` | `AvsluttSak` |
| `SettSaksansvarlig` | `SettSaksansvarlig` |

Prerequisites, uttrykt mot facts:

| Operasjon | Prerequisite |
|---|---|
| `OpprettSak` | ingen |
| `RenderDokument` | dokumentet er HTML-template; saksnummer finnes hvis malen bruker det |
| `OpprettJournalpost` | saken har arkiv-id; hoveddokumentet er `ok` |
| `LeggTilVedlegg` | journalposten er opprettet og ikke journalført |
| `Journalfør` | alle dokumenter på journalposten er `ok` |
| `SettEkspedert` / `KlargjørForEkspedering` | alle dokumenter på journalposten er `ok` |
| `AvventJournalført` | journalposten er satt til `E` eller `F` |
| `Avskriv` | journalposten er journalført og er av type `I` |
| `SettSaksansvarlig` | saken har arkiv-id |
| `AvsluttSak` | **alle andre operasjoner på saken er terminalt ok** (D4) |

---

## Domenefunksjoner

Rene, uten IO. Erstatter `planlegg_neste_handling`.

```rust
/// Ren funksjon av command payload. Kalles én gang, ved innlesing.
fn dekomponer(command: &Command) -> Vec<Operasjonsspesifikasjon>

/// Gjelder alle operasjoner unntatt AvsluttSak. Facts alene.
fn vurder(op: &Operasjon, facts: &SakMedBarn) -> Beslutning

/// AvsluttSak er sitt eget tilfelle (D4).
fn vurder_avslutt_sak(
    op: &Operasjon,
    facts: &SakMedBarn,
    sosken: &[OperasjonSammendrag],
) -> Beslutning

/// Styrer om operasjonen går gjennom `sendt`-fasen (D7).
fn muterer_arkivet(t: Operasjonstype) -> bool

enum Beslutning {
    Utfor,
    Blokkert(BlockedReason),
    AlleredeUtfort,
    Ugyldig(DomainViolation),
}
```

Applikasjonslaget dispatcher på operasjonstype og henter søskenoperasjoner **kun** for `AvsluttSak`.
Den generelle stien forblir facts-only og trivielt testbar.

`BlockedReason` og `DomainViolation` beholdes fra dagens domenekode, inkludert `as_code()` og
`safe_detail()` — de stabile kodene er allerede i bruk i dashboards.

---

## Databaseskjema

Ren migrasjonshistorikk. Én ny base-migrasjon.

```sql
CREATE TYPE entitet_type AS ENUM ('sak','journalpost','dokument');

-- Identitet. Master for skuffen_id. Erstatter id_mapping.
CREATE TABLE entitet (
  skuffen_id       UUID PRIMARY KEY,
  entitet_type     entitet_type NOT NULL,
  client_reference UUID UNIQUE,
  arkiv_id         TEXT,
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (entitet_type, arkiv_id),
  CHECK  (client_reference IS NOT NULL OR arkiv_id IS NOT NULL)
);

-- Mottaksjournal + idempotency-hovedbok.
CREATE TABLE kommando (
  command_id     UUID PRIMARY KEY,
  correlation_id UUID,
  kommandotype   TEXT NOT NULL,
  payload        JSONB NOT NULL,
  mottatt_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  dispatchet_at  TIMESTAMPTZ,          -- idempotency-milepæl (D24)
  dekomponert_at TIMESTAMPTZ
);

CREATE TYPE operasjon_status AS ENUM (
  'blokkert','klar','kjorer','sendt','retry_venter',
  'ok','feilet','krever_avklaring'
);

CREATE TABLE operasjon (
  operasjon_id    UUID PRIMARY KEY,
  command_id      UUID NOT NULL REFERENCES kommando(command_id),
  operasjonstype  TEXT NOT NULL,
  entitet_id      UUID NOT NULL REFERENCES entitet(skuffen_id),      -- svak FK (D28)
  sak_id          UUID NOT NULL REFERENCES sak_tilstand(sak_id),     -- partisjonsnøkkel (D29)
  status          operasjon_status NOT NULL DEFAULT 'blokkert',
  attempt_no      INT  NOT NULL DEFAULT 0,
  neste_forsok_at TIMESTAMPTZ,
  blokkert_av     UUID REFERENCES operasjon(operasjon_id),
  siste_detalj    TEXT,
  sendt_at        TIMESTAMPTZ,
  ferdig_at       TIMESTAMPTZ,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (command_id, operasjonstype, entitet_id)
);

CREATE INDEX ON operasjon (neste_forsok_at) WHERE status IN ('klar','retry_venter');
CREATE INDEX ON operasjon (sak_id)          WHERE status = 'blokkert';
CREATE INDEX ON operasjon (command_id);

CREATE TABLE operasjon_forsok (
  operasjon_id UUID NOT NULL REFERENCES operasjon(operasjon_id),
  attempt_no   INT  NOT NULL,
  executor_id  TEXT NOT NULL,
  startet_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
  avsluttet_at TIMESTAMPTZ,
  utfall       TEXT,
  detalj       TEXT,
  PRIMARY KEY (operasjon_id, attempt_no)
);
```

### State-tabellene

Beholder navn og tilstandsmaskiner. Endres slik:

- PK-ene blir FK til `entitet(skuffen_id)`.
- `saksnummer`, `sikri_id`, `journalpostnummer` fjernes (D25).
- Attributter materialiseres ved dekomponering (D26):
  - `sak_tilstand`: `sakstittel`, `arkivdel`, `ordningsverdi`
  - `journalpost_tilstand`: `tittel`, `dokument_dato`, `tilgjengelighet`, `korrespondanseparter`
  - `dokument_tilstand`: `tittel`, `filtype`, `dokument_referanse`, `rekkefolge`,
    `er_hoveddokument`, `mal_referanse`, `felter`, `rendered_dokument_referanse`
- `dokument_tilstand` får `UNIQUE (journalpost_id, rekkefolge)` og
  `CHECK (er_hoveddokument = (rekkefolge = 0))` (D27).
- `journalpost_tilstand.tilstand` utvides med `ekspedert` for observert `E`.

### Egenskaper verdt å merke seg

- `UNIQUE (command_id, operasjonstype, entitet_id)` gjør dekomponering idempotent **strukturelt**.
  En replay setter inn null rader, og `rows_affected` er signalet om det var første gang. Ingen
  egen flaggkolonne trengs; `utfores_venter_publisert_at` utgår.
- Kommandostatus finnes **ikke** som kolonne — den er et fold over `operasjon` (D14). Riktig
  normalisering, men en `GROUP BY command_id` per publisert event. Kan materialiseres senere hvis
  det viser seg å koste; ikke gjør det nå.
- `blokkert_av` skrives av evalueringspasset og er alltid ferskt. Derivert debughjelp, aldri
  autoritativ.
- Hele dekomponeringen — entitet, state og alle operasjonsrader — skjer i **én transaksjon**. I dag
  er dette 7+ uavhengige autocommit-statements, og en delvis skriving er permanent PK-violation ved
  redelivery.

---

## NATS-kontrakt

```
Stream: arkiv_status     subjects: arkiv.status.>

arkiv.status.<command_id>.command
arkiv.status.<command_id>.operasjon.<operasjon_id>
```

| Klienten vil ha | Subscription |
|---|---|
| Bare utfallet | `arkiv.status.<cmd>.command` |
| Bare operasjonsdetaljer | `arkiv.status.<cmd>.operasjon.>` |
| Full logg for kommandoen | `arkiv.status.<cmd>.>` |
| Alt (dashboard/audit) | `arkiv.status.>` |

Kommandoeventet ligger på `.command` og ikke på `arkiv.status.<cmd>` fordi `arkiv.status.<cmd>.>`
ikke matcher `arkiv.status.<cmd>` selv. Ett ekstra token kjøper én subscription for hele historikken.

Hendelser:

- Kommando: `mottatt | validert | avvist | utfores | fullfort | feilet`
- Operasjon: `forsok_feilet | ok | feilet | krever_avklaring | varsel`

Nye typer i `lib-schemas` (`landdyrtilsyn-libs`, git-dep). `SkuffenStatusEventV1` er
`#[serde(deny_unknown_fields)]` og kan ikke utvides — den erstattes. Utadgående meldinger skal
fortsatt være sanitiserte: statiske, klientvennlige strenger, aldri intern `detail` eller
stacktrace.

Uendret: `arkiv.arkiver`, `arkiv.command.inbox.*`, `arkiv.command.ready.*`, `arkiv.request.*`.
Slettes: `arkiv.command.done.*` og stream `arkiv_command_done` (D34).

---

## Slettelisten

Blank slate. Ingenting av dette skal overleve i omskrevet form «for sikkerhets skyld».

**Tabeller:** `command_execution`, `command_execution_attempt`, `id_mapping`, `tilstand_historikk`.

**Streams:** `arkiv_command_done`.

**Domain:**
- `eksekvering::tilstand::planlegg_neste_handling`, `CommandStateDecision`, `ArkivOperasjon`
- `src/domain/src/model/operasjon/` — død stub fra 2024, aldri koblet inn

**Application:**
- `services/eksekver_kommando.rs` + hele `eksekver_kommando/`-mappen
- `services/registrer_i_eksekveringssystem.rs`, `services/execution_registration.rs`
- `services/command_state_decision.rs`, `services/eksekvering_worker.rs`
- `services/reevaluer_ventende_kommandoer.rs` + `ports/ventende_kommando_wakeup_port.rs` —
  hendelsesdrevet wake-up erstattes av et periodisk evalueringspass over `blokkert`-operasjoner
- `ports/command_execution_port.rs`
- `EksekveringKvitteringPublisher`
- Dobbeltimplementasjonen `CommandStatusPublisher` / `EksekveringStatusPublisher` — kollapses til én
- `logg_overgang` fra entity-porten (~7 kallsteder i handlers)

**Infrastructure:**
- `adapter/postgres_execution_store.rs`
- `adapter/nats_done_publisher.rs`
- `adapter/nats_eksekvering_status_publisher.rs` (duplikat av `nats_status_publisher.rs`)

**Dokumentasjon:** `docs/command_executor.md` og `docs/execution_v2_design.md` er slettet.
`.agent/skills/arkivfag/resources/journalpost/inngaaende.md` og `internt_notat.md` er rettet — de
dokumenterte opprettelse direkte i `J` med `avskrivDirekte=true`, altså buggen i D17.
`utgaaende.md` var allerede korrekt: den beskriver `R → E → J` for uten-utsending, som er det D19/D20
sier. Det var koden som avvek, ikke dokumentet.

**ADR:** SKU-0001, SKU-0007 og SKU-0010 blir helt eller delvis superseded. Kjør
`cargo run -p adr-fmt -- --critique <ID>` på alle tre før noe skrives.

---

## Kjente defekter som fikses

Tre `#[ignore]`-merkede regresjonstester beskriver ønsket oppførsel og feiler i dag. Å fjerne
`#[ignore]`-linja er definition of done for hver av dem.

| Test | Defekt | Fikses av |
|---|---|---|
| `retry_etter_dispatch_feil_skal_ikke_kvittere_ok_for_udispatchet_kommando` | Idempotency-markøren skrives før dispatch. Dispatch-feil + klient-retry gir OK-kvittering for en kommando som aldri ble sendt, aldri validert, aldri eksekvert. | D24 — `dispatchet_at` som milepæl |
| `db_feil_etter_arkivskriv_skal_ikke_gi_duplikat_sak` | DB-feil etter vellykket Sikri-skriv gir duplikat sak i arkivet. | D8/D9 — to-fase `sendt` + `krever_avklaring` |
| `done_uten_operasjon_skal_likevel_vekke_blokkerte_kommandoer` | `Done`-stien trigger ingen wake-up; blokkerte kommandoer forblir blokkert, og det finnes ingen periodisk rescan som redder dem. | Evalueringspass erstatter hendelsesdrevet wake-up |

I tillegg fikses, uten at det finnes tester for det i dag:

- Ikke-transaksjonell registrering (7+ autocommit-statements, orphan-rader mulig ved crash).
- Den posisjonelle hoveddokument-koblingen (D27).
- At inngående og internt notat opprettes i `J` slik at vedlegg ikke kan legges til (D17).
- At `AvsluttSak` og `SettSaksansvarlig` ikke er idempotente i det hele tatt (D24).
- At `AvsluttSak` kan lukke en sak der utgående journalposter ennå ikke er journalført (D20).

---

## Faseplan

| Fase | Innhold | Ferdig når |
|---|---|---|
| 0 | ADR for operasjonsmodellen. `--critique` på SKU-0001/0007/0010, flytt superseded til `stale/` | `--lint` uten errors |
| 1 | Domain: `Operasjonstype`, `dekomponer`, `vurder`, `vurder_avslutt_sak`, `muterer_arkivet`. Null IO | unit-tester dekker alle 7 operasjonslistene og alle prerequisites |
| 2 | Nytt skjema, én migrasjon, ren historikk | repo-tester mot lokal Postgres |
| 3 | Dekomponering ved innlesing av validert kommando, i én transaksjon, med materialisering til state | ingen orphan-rader mulig ved crash |
| 4 | Executor på operasjoner, to-fase `sendt`, `krever_avklaring` | `db_feil_etter_arkivskriv_skal_ikke_gi_duplikat_sak` grønn |
| 5 | Periodisk evalueringspass erstatter wake-up-tjenesten | `done_uten_operasjon_skal_likevel_vekke_blokkerte_kommandoer` grønn |
| 6 | Statuskontrakt: nye `lib-schemas`-typer, subject-skjema, msg-id, terminal-semantikk | integration-test på begge subject-dybder |
| 7 | `hent_journalpost` i `sikri_client` + `AvventJournalført` | integration-test mot fake gateway |
| 8 | `dispatchet_at`-idempotency i ingest | `retry_etter_dispatch_feil_...` grønn |
| 9 | Slett legacy, skriv om dokumentasjon og arkivfag-ressurser | `cargo clippy --all-targets --all-features`, full integration-suite |

Fase 1 og 2 kan gå parallelt. Fase 7 og 8 er uavhengige av 4–6.

`GET /api/Archive/HentJournalpost` finnes i Sikri-swagger, men er ikke wrappet i
`crates/sikri_client/src/api.rs` i dag. Det er ny kode i fase 7.

---

## Åpne punkter

Ingen blokkerende. Følgende er bevisst utsatt:

- **Idempotency key fra Sikri.** Uten den er `OpprettSak` ikke rekonsilierbar ved ukjent utfall
  (D9) — det finnes ikke noe søke-endepunkt å matche mot. Tas opp med arkivutvikling til en senere
  versjon.
- **Poll-intervall for `AvventJournalført`** er satt til én time som utgangspunkt og skal tunes mot
  observert RPA-latens.
- **24-timersvarselet** (D11) er advisory i v1. Om det skal kunne eskalere til terminal er utsatt.
- **Sak-visning** (D35) er droppet i denne omgang.
- **Materialisert kommandostatus** hvis foldet over `operasjon` viser seg å koste for mye.
