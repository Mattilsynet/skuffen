## Eksempel-requests (kopier/lim inn)

Sjekk at tjenesten svarer:

```bash
nats request skuffen.ready '"ping"'
```

Opprett sak (gyldig JSON for `arkiv.arkiver`):

```bash
nats request arkiv.arkiver '[
  {
    "command_id": "11111111-1111-4111-8111-111111111111",
    "correlation_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    "payload": {
      "OpprettSak": {
        "client_reference": "22222222-2222-4222-8222-222222222222",
        "sakstittel": "Manual test sak",
        "arkivdel": "Tilsynsdivisjonene",
        "saksbehandler_id": "Z12345",
        "saksbehandler_enhet": "42",
        "ordningsverdi": "123",
        "tilgang": null
      }
    }
  }
]'
```

Sekvens: opprett sak -> opprett internt notat journalpost -> avslutt sak.

```bash
nats request arkiv.arkiver '[
  {
    "command_id": "11111111-1111-4111-8111-111111111111",
    "correlation_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    "payload": {
      "OpprettSak": {
        "client_reference": "22222222-2222-4222-8222-222222222222",
        "sakstittel": "Manual test sak",
        "arkivdel": "Tilsynsdivisjonene",
        "saksbehandler_id": "Z12345",
        "saksbehandler_enhet": "42",
        "ordningsverdi": "123",
        "tilgang": null
      }
    }
  },
  {
    "command_id": "33333333-3333-4333-8333-333333333333",
    "correlation_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    "payload": {
      "OpprettInterntNotatJournalpost": {
        "felles": {
          "client_reference": "44444444-4444-4444-8444-444444444444",
          "tittel": "Manual test internt notat",
          "dokument_dato": "2025-01-01",
          "saksbehandler": "Z12345",
          "saksbehandler_enhet": "42",
          "tilgang": null,
          "dokumenter": [
            {
              "client_reference": "55555555-5555-4555-8555-555555555555",
              "tittel": "Vedlegg",
              "form": {
                "Bytes": {
                  "dokument_referanse": "66666666-6666-4666-8666-666666666666",
                  "filtype": "PDF"
                }
              }
            }
          ],
          "sak_key": {
            "ClientReference": "22222222-2222-4222-8222-222222222222"
          },
          "kildesystem": null
        }
      }
    }
  },
  {
    "command_id": "77777777-7777-4777-8777-777777777777",
    "correlation_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    "payload": {
      "AvsluttSak": {
        "sak_key": {
          "ClientReference": "22222222-2222-4222-8222-222222222222"
        }
      }
    }
  }
]'
```

Merk: for journalpost-kommandoer maa `dokument_referanse` vaere lastet opp paa
`arkiv.arkiver.media` foer kommandoen sendes.

## Manuell E2E test via lokal NATS

Dette setter opp lokal Postgres + NATS, starter Skuffen lokalt og sender en request over NATS som treffer Skuffen og kaller Sikri.

Krever:
- docker
- cargo
- nats (CLI)
- nats-server (anbefalt) eller Docker image for NATS

Integrasjonstester (alltid med mocket Sikri):

```bash
cargo test -p skuffen-integration-tests -- --nocapture
```

Manuelle tester via egen kommando:

```bash
# kreves for send-sequence
export SIKRI_SAKSBEHANDLER_ID="<saksbehandler-id>"
export SIKRI_SAKSBEHANDLER_ENHET="<saksbehandler-enhet>"

# ping klar-status
cargo run -p skuffen-integration-tests --bin skuffen-manual -- ready

# send en komplett sekvens (inkluderer media-upload)
cargo run -p skuffen-integration-tests --bin skuffen-manual -- send-sequence

# foelg status for kommandoer (henter historikk + live oppdateringer)
cargo run -p skuffen-integration-tests --bin skuffen-manual -- watch-status <command-id-1> <command-id-2> <command-id-3>

# bruk aktiv nats-cli context (eller spesifikk context)
cargo run -p skuffen-integration-tests --bin skuffen-manual -- ready --context arkiv-test

# send-sequence skriver ut en ferdig watch-status kommando du kan kopiere direkte
```

## Git hooks (pre-push)

For å hindre at interne IDs eller secrets havner i historikken, installer pre-push hooken:

```bash
scripts/git-hooks/install.sh
```

Hvis du faar "permission denied", bruk:

```bash
chmod +x scripts/git-hooks/install.sh
./scripts/git-hooks/install.sh
```

Hooken bruker `gitleaks` hvis den er installert, og i tillegg en felles liste over
forbudte patterns fra `scripts/git-hooks/forbidden-patterns.txt` (i repoet).

## Telemetry / tracing

Skuffen logger requests/responses mot Sikri via `tracing` (`target="sikri.http"`).

For aa faa spans i GCP Trace maa OTLP-export vaere satt opp i runtime:

```bash
OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=<otlp-grpc-endpoint>
# evt fallback:
OTEL_EXPORTER_OTLP_ENDPOINT=<otlp-grpc-endpoint>
```

Hvis OTLP-endpoint ikke er satt, logger tjenesten fortsatt structured logs, men spans eksporteres ikke til trace-backend.

# skuffen

`skuffen` er en arkiveringstjeneste som ligger mellom interne systemer i Mattilsynet og Sikri sitt arkivsystem.

Tjenesten abstraherer bort et komplekst og upålitelig eksternt arkiv-API, og tilbyr et stabilt, asynkront og meldingsbasert grensesnitt for arkivering av saker og journalposter.

---

## Overordnet ansvar

Skuffen:
- mottar **kommandoer** fra interne tjenester
- oversetter disse til én eller flere **arkivoperasjoner**
- utfører operasjonene mot Sikri sitt arkiv når mulig
- lagrer og eksponerer status og resultater
- returnerer kun feil ved irrecoverable conditions

Tjenesten er *best effort* av design.

---

## Arkitektur

### Lagdeling

Tjenesten følger en strikt hexagonal arkitektur med CQRS-skille:
-Infrastructure
-Application
-Domain

- **Domain**
  - Ren forretningslogikk
  - Ingen eksterne avhengigheter
  - Regler for sak, journalpost og tilstandsovergang

- **Application**
  - Orkestrering
  - Porter (traits) for repositories, queries og eksterne adaptere

- **Infrastructure**
  - NATS
  - PostgreSQL
  - Sikri arkiv-API
  - Blob storage
  - Idempotency og køhåndtering

---

## Kommunikasjon

All integrasjon skjer over **NATS**.

- Skuffen kjører i én NATS account
- Andre tjenester kan ligge i andre accounts
- Ingen JWT over NATS
- Klienter publiserer meldinger og lytter på svar

### NATS subjects og streams

Request-reply (skriv / kommandoer):
- `arkiv.arkiver` (kommandoer). Request: `Vec<CommandEnvelope<Command>>`. Reply: `ArkiveringKvittering`.
  - OK: `{ "Ok": { "command_ids": ["<uuid>"] } }` betyr at hele batchen er mottatt og akseptert for prosessering.
  - Error: `{ "Error": { "message": "..." } }` betyr at hele batchen er avvist. Execution-resultat kommer via status-events, ikke request-reply.
- `arkiv.admin` (administrative funksjoner).

Request-reply (les / queries):
- `arkiv.request.sak.hent` — hent sak. Request: `HentSakQuery` med `SakKey::ClientReference(uuid)` eller `SakKey::ArkivId(Saksnummer)`. Reply: `NatsResponse<SakResponse>`.
- `arkiv.request.journalpost.hent` — hent journalpost. Request: `HentJournalpostQuery` med `JournalpostKey::ClientReference(uuid)` eller `JournalpostKey::JournalpostId(journalpost_id)`. Reply: `NatsResponse<JournalpostResponse>`. NB: subjectet er koblet opp, men backing repository er foreløpig fake/testdata.
- `arkiv.request.bruker.mt_enheter` — bruker/MT-enheter. Request: `{}`. Reply: `NatsResponse::Error { message: "Not implemented" }` inntil kontrakt og backing implementation er avklart.

Query replies bruker `NatsResponse<T>`:

```json
{"status":"Ok","payload":{}}
```

```json
{"status":"Error","payload":{"message":"Not implemented"}}
```

Wire-shape for query keys er tagged JSON fra `lib-schemas`, for eksempel:

```json
{"key":{"type":"clientReference","value":"00000000-0000-0000-0000-000000000000"}}
```

```json
{"key":{"type":"journalpostId","value":"12345"}}
```

JetStream (til klienter) — én statusstrøm, `arkiv_status`, retention 180 dager.
Strømmen **er** loggen; en klient som vil ha historikken lager en consumer med `DeliverPolicy::All`.

| Subject | Payload |
| :-- | :-- |
| `arkiv.status.<commandId>.command` | `SkuffenKommandoStatusV1` |
| `arkiv.status.<commandId>.operasjon.<operasjonId>` | `SkuffenOperasjonStatusV1` |

| Klienten vil ha | Subscription |
| :-- | :-- |
| Bare utfallet | `arkiv.status.<cmd>.command` |
| Bare operasjonsdetaljer | `arkiv.status.<cmd>.operasjon.>` |
| Full logg for kommandoen | `arkiv.status.<cmd>.>` |
| Alt (dashboard/audit) | `arkiv.status.>` |

`terminal: true` betyr at **utfallet er avgjort**, ikke at flere meldinger er utelukket.
Operasjonsmeldinger kan fortsette etterpå, fordi søskenoperasjoner kjører videre best effort.

Alle JetStream-streams og `arkiv_media` object store konfigureres med `num_replicas = 3`.

Interne JetStreams (med `commandId` i subject for enklere debugging, retention 180 dager):
- Stream: `arkiv_command_inbox` (subject: `arkiv.command.inbox.<entity>.<commandId>`)
- Stream: `arkiv_command_ready` (subject: `arkiv.command.ready.<entity>.<commandId>`)

Durable consumers:
- `validator` leser `arkiv_command_inbox` med explicit ack og `num_replicas = 3`.
- `executor` leser `arkiv_command_ready` med explicit ack og `num_replicas = 3`.

`<entity>` er `sak` eller `journalpost`.

---

## Eksekvering av kommandoer

Se design og domenelogikk i `docs/execution_v3_design.md` og ADR
[SKU-0016](docs/adr/skuffen/SKU-0016-operasjonsbasert-eksekvering.md).

En kommando dekomponeres én gang til en flat liste av **operasjoner**. En operasjon er ett
arkivkall, med egen id, egen status, egen retry og egen statuslinje utad.

## Retry- og eksekveringsmodell

- NATS `arkiv.command.inbox.*` brukes til validering. Meldingen ACKes kun når validering er ferdig;
  recoverable og blokkerte utfall NAKes for redelivery.
- NATS `arkiv.command.ready.*` brukes til dekomponering. Meldingen ACKes når kommandoen er
  dekomponert til operasjonsrader — alt i én transaksjon, så en replay setter inn null rader.
- Eksekvering styres av en intern worker som plukker operasjoner i `klar`, eller `retry_venter` med
  forfalt frist. Ett periodisk evalueringspass frigjør `blokkert`-operasjoner når fakta tilsier det.
- Skriveoperasjoner commiter `klar → sendt` **før** arkivkallet, og `sendt → ok` med arkivsvar og
  faktaoppdatering i én transaksjon etterpå. En operasjon funnet i `sendt` ved oppstart har ukjent
  utfall og går til `krever_avklaring` for manuell opprydding.
- Recoverable feil retryes for alltid med eksponentiell backoff opp til én gang per døgn. Kun
  irrecoverable feil blir terminalt `feilet`. Terminal feil krever positivt treff i regelsettet;
  se [SKU-0017](docs/adr/skuffen/SKU-0017-terminal-feil-krever-positivt-treff.md) og
  «Feilhåndtering» lenger ned.
- Eksekvering er enleder: én instans holder en Postgres advisory lock og plukker operasjoner.
  Låsen ligger på en connection som er tatt ut av poolen, så den slippes først når leasen droppes.
- Dekomponering skjer én gang. Operasjonslisten er en ren funksjon av command payload, og det
  finnes ingen re-planlegging. Executor leser materialiserte attributter fra tilstandstabellene og
  rører aldri payloaden.

## Runtime-prioritering

- `command_listener`, `media_listener` og `health_check` regnes som kritiske for opptak. De restartes internt, men hvis de stopper eller feiler mer enn 3 ganger på rad, avsluttes prosessen slik at Cloud Run kan restarte instansen.
- `validation_listener`, `execution_listener`, `execution_worker`, `query_listener` og `ready_replier` regnes som degradérbare: hvis de stopper eller feiler, logger Skuffen feilen og holder prosessen i live. `query_listener` dekker `arkiv.request.sak.hent`, `arkiv.request.journalpost.hent` og `arkiv.request.bruker.mt_enheter`.

---

## Data- og meldingsmodell

### Sekvens

En **sekvens** er en liste av kommandoer som hører logisk sammen.

```json
[
  {
    "command_id": "00000000-0000-0000-0000-000000000001",
    "correlation_id": "00000000-0000-0000-0000-000000000010",
    "payload": { "OpprettSak": { } }
  },
  {
    "command_id": "00000000-0000-0000-0000-000000000002",
    "correlation_id": "00000000-0000-0000-0000-000000000010",
    "payload": { "OpprettInngåendeJournalpost": { } }
  }
]
```

### Kommando

En kommando er en instruksjon fra et klientsystem.
	•	Asynkron
	•	Idempotent
	•	Mapper til én eller flere operasjoner

Eksempler:
	•	OpprettSak
	•	OpprettInngåendeJournalpost
	•	OpprettUtgåendeJournalpost
	•	AvsluttSak

Kommandoer er del av den offentlige kontrakten.



### Operasjon

En operasjon er en konkret handling mot arkivet.
	•	1:1 mapping mot et kall i Sikri sitt API
	•	Utføres sekvensielt
	•	Har egen status

Eksempler:
	•	OpprettJournalpost
	•	LeggTilVedlegg
	•	Journalfør
	•	Avskriv
	•	SendUt

Operasjoner er interne.


### Query

En query er et rent lesekall.
	•	Synkron
	•	Leser kun flate DTO-er
	•	Laster aldri domene-entiteter

Eksempler:
	•	`arkiv.request.sak.hent` — hent sak
	•	`arkiv.request.journalpost.hent` — hent journalpost
	•	`arkiv.request.bruker.mt_enheter` — bruker/MT-enheter, foreløpig `Not implemented`
	•	Hent status for kommando eller sekvens

### Mapping

Skuffen har en stabil intern ID(skuffen-id) for alle entiteter. Denne er skjult for omverden.
Klienter sender med sin egen client-reference for entiteter som kan brukes til å referere til entieter. Eks. hvis arkivet et nede så kan man sende inn opprett sak<client-reference: 123> og senere sende inn OpprettJournalpost<client-reference: abc, sak: 123>.
Skuffen aksepterer også arkivet sine IDer.

Det finnes altså en mapping mellom alle 3:

client-reference <-> skuffen-id <-> arkiv-id

### Tilstandsmodell

#### Sak
	•	Under behandling
	•	Avsluttet


Avslutting kan ikke skje før alle journalposter er ferdig behandlet; avsrevet og journalført

#### Journalpost (forenklet)


Inngående / interne:
Opprettet → Journalført → Avskrevet

Utgående uten utsending:
Opprettet → Journalført




## Feilhåndtering

Skuffen følger best effort.
	•	Midlertidige feil lagres og retries
	•	Arkiv-nedetid skal ikke påvirke klienter
	•	Kommandoer avvises kun ved:
	•	valideringsfeil
	•	brudd på domeneinvarianter
	•	irrecoverable tekniske feil

Feil klassifiseres som:
	•	Recoverable (Prøves på nytt)
	•	Irrecoverable (Gir feilmelding tilbake til client, og stopper retries)

**Terminal feil krever positivt treff** (SKU-0017). `sikri_client` klassifiserer mot et eksplisitt
regelsett, og bunnen er `Recoverable`: en feil vi ikke har en regel for, retryes til noen legger
inn en. `401` og `403` er recoverable — et rotert passord skal ikke terminere hver operasjon som er
underveis, siden `feilet` er monotont og ikke kan trekkes tilbake. `404` er irrecoverable.
Body-regler går foran statusregler, så kjent feiltekst terminerer selv der statuskoden alene ville
gitt retry.

Klassifiseringen bæres som en typet feil hele veien. `sikri_client` eier kode og klientvendt
melding; adapterne i `infrastructure` legger på klientvendt feilkode; `application` videreformidler
uten å tolke. Feilens felter har hver sin mottaker: `kode` og intern detalj går til
`operasjon.siste_detalj`, mens `melding` og `error_code` går til klienten på statusstrømmen.
Underliggende feiltekst — sqlx-feil, rå Sikri-body — når aldri klienten.

Nye `sikri_*`-koder fanges av en dekningstest: hver kode må ha en oversettelse til en klientvendt
feilkode før den kan nå en klient.



## Idempotens og sporbarhet
	•	Alle kommandoer har stabil kommandoId
	•	Idempotency-log hindrer dobbel utførelse
	•	Status kan alltid hentes i etterkant


## Kontraktsdeling

DTO-typene ligger i `landdyrtilsyn-libs/lib-schemas`; latest source kan sees her:
https://github.com/Mattilsynet/landdyrtilsyn-libs/tree/master/lib-schemas/src/skuffen

Skuffen følger latest git HEAD for `lib-schemas`/`lib-nats` i `Cargo.toml`, mens `Cargo.lock` er resolved build boundary for konkret bygg.

Disse utgjør den stabile kontrakten.

Infrastruktur oversetter mellom offentlig kontrakt og intern domene-modell.

Målgruppe

Primært:
	•	Utviklere i Mattilsynet som integrerer mot arkiv

Sekundært:
	•	Eksterne som vil forstå arkitekturen

Repoet er public, men tjenesten er bygget for intern bruk.
