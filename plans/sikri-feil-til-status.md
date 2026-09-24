# Plan: presise Sikri-feil i statusstrømmen

Status: Implementert. Intern kode ble `sikri_unresolved_journalposter` etter
operatørens navnevalg, ikke `sikri_sak_kan_ikke_avsluttes` som §3.1 foreslo.
Reparasjon av terminalt feilede forsøk håndteres senere gjennom
admin-grensesnittet og er utenfor leveransen (§5).

## 1. Mål og avtalte beslutninger

Klienten skal få en terminal, forståelig beskjed når Sikri avviser avslutning
fordi saken har uavskrevne restanser. Kjente feil ved tomt vedleggsinnhold skal
også avsluttes terminalt, ikke retryes som generell arkivnedetid.

Avtalt med operatøren:

| Tilfelle | Klassifisering | Endring |
|---|---|---|
| R1: saken har uavskrevne restanser | Irrecoverable | Ny positiv feilregel og presis melding |
| R3: vedleggslisten har dokumentfiler uten innhold | Irrecoverable | Utvid eksisterende regel for manglende dokumentinnhold |
| R2: sak ikke funnet | Uendret | Behold eksisterende 404-håndtering |
| R4: ressurs ikke synlig / org-enhet ikke funnet | Uendret | Ingen særregel eller ny kode |

R1 skal ha **nøyaktig** denne klientteksten, uten tillegg om ny innsending:

> Saken har journalposter som ikke er avskrevet (restanser) og kan ikke avsluttes.

Saksnummer, klientreferanse og sporingsidentifikatorer skal være strukturerte
felt fra Skuffens kommandokontekst, ikke tekst utledet fra Sikris feilsvar.

### Avgrensning

- Ingen generell omklassifisering av 500, 400, 401, 403, 404, 429 eller 503.
- Ingen automatisk avskriving, journalføring eller ny kommando.
- Ingen endring av søskenkravet for avslutning eller gjenåpning av terminale
  feil. Reparasjon gjennom admin-grensesnittet kommer i en separat leveranse.
- Ingen implementasjon av `FerdigbehandleJournalpost` i denne leveransen.
- Ingen nye oppslag i arkivet for å finne blokkerende journalposter.
- Ingen endring av loggnivå i produksjon eller lagring av produksjonslogger.
- Ingen nye public feilkoder, schema-release eller databasekolonner i
  grunnleveransen. Dersom implementasjonen krever dette, avklar før utvidelse.

## 2. Grunnlag og nåværende oppførsel

Operasjonell undersøkelse har identifisert følgende feilformuleringer fra
Sikri-grensesnittet:

- `Det finnes {N} ikke avskrevne restanser` ved HTTP 500 fra
  `SetStatusForArkivSak` ved avslutning. `{N}` er et positivt heltall.
- `Vedleggslisten har dokument-filer som mangler innhold` ved HTTP 500 fra
  `LeggTilVedleggPaaJournalpost`.

Dette dokumentet inneholder bare feilformuleringene som trengs for reglene,
ikke loggutdrag, hendelsesidentifikatorer, request-parametre eller stacktraces.
Testdata skal bygges syntetisk; ingen produksjonsbody skal kopieres til repoet.
Feilformuleringen for restanser er bekreftet fra en annen integrasjons logsvar;
den opprinnelige Skuffen-hendelsens råbody er ikke tilgjengelig. Lengden på en
body er ikke bevis for at innholdet er identisk.

Kallveien i dag:

1. `crates/sikri_client/src/api.rs`: `ensure_success` leser feilsvaret.
2. `crates/sikri_client/src/error_mapping.rs`: `SikriFeil::fra_http` velger
   klassifisering, intern kode og trygg klientmelding. Ukjent 500 blir retrybar.
3. `src/infrastructure/src/command/adapter/sikri_arkiv_gateway.rs` oversetter
   til domenefeil med public feilkode.
4. `src/application/src/command/services/eksekver_operasjon.rs` lagrer retry
   eller terminal feil, og publiserer operasjonsstatus.
5. Samme tjeneste folder kommandoens utfall, men erstatter i dag terminal
   feilårsak med generisk melding og `ProcessingFailed` på kommandonivå.

SKU-0017 (terminal feil krever positivt treff) krever eksplisitte regler og
tester gjennom kallveien. SKU-0016 (operasjonsbasert eksekvering) krever at
terminal feil ikke reverseres, og at øvrige operasjoner fortsetter best effort.
SKU-0020 (executor eier beslutning og publisering) tillater gjentatte
statushendelser; `terminal` betyr avgjort utfall, ikke «siste melding».

## 3. Feilklassifisering

### 3.1 R1: uavskrevne restanser

- Innfør intern kode `sikri_unresolved_journalposter` i `sikri_client`.
- Legg koden i `ALLE_SIKRI_KODER`, listen over produserbare Sikri-koder, og
  meldingskatalogen med den avtalte teksten fra §1.
- Positivt treff skal gjelde den kjente meldingen ved HTTP 500. Ikke match
  bare ordene «restanse», «journalpost» eller «kan ikke avsluttes».
- For den nye regelen: les `errorMessage` fra JSON-envelope og gjenkjenn
  `Det finnes <positivt heltall> ikke avskrevne restanser`. Ikke la en kopi
  av teksten i ekkoede input-parametre eller stacktrace alene terminere.
  Manglende/ugyldig JSON eller ukjent formulering faller tilbake til dagens
  regler. Ikke refaktorer alle eksisterende matchere som del av dette.
- Hold klassifisering og intern kode i samsvar; samme treff skal ikke gi
  terminal klassifisering med generell retry-melding eller omvendt.
- Map koden til `StatusErrorCode::PrerequisitePending` i infrastrukturlaget.
  På wire heter koden `PREREQUISITE_PENDING`; at ingen nye forsøk kommer,
  uttrykkes av `hendelse = feilet` og `terminal = true`.
- Oppdater begge adapteres kodemapping/dekning: `sikri_arkiv_gateway.rs` og
  `sikri_command_state_repo.rs`. Valideringsadapterens eksisterende oppførsel
  for øvrige koder skal ikke endres.

### 3.2 R3: manglende innhold i vedlegg

- Gjenbruk `sikri_missing_document_content`, den eksisterende interne koden
  for dokumentfiler uten innhold, og public `INVALID_REQUEST`.
- Behold eksisterende klienttekst:
  «Sikri/Elements avviste forespørselen fordi dokumentet mangler innhold.»
- Legg til den kjente vedleggsformuleringen ved HTTP 500, og bevar treff for
  `Ny journalpost har dokument-filer som mangler innhold`.
- Avgrens den nye vedleggsvarianten til `errorMessage`, som for R1. Teksten
  bare i input eller stacktrace skal ikke gi nytt terminalt treff. Behold
  eksisterende journalpostregel uten en generell refaktorering.
- Ikke utvid til 503 eller andre statuser uten et nytt bekreftet tilfelle.

## 4. Status til klienten: årsak og identifikatorer

### 4.1 Behold wire-kontrakten

Den resolved interne git-avhengigheten `lib-schemas` tag `1.7.2` definerer:

| Subject | Innhold som brukes her |
|---|---|
| `arkiv.status.<command_id>.command` | `message`, `error_code`, `terminal`, `command_id`, `correlation_id`, `saksnummer`, `sak_client_reference`, `timestamp` |
| `arkiv.status.<command_id>.operasjon.<operasjon_id>` | Samme feilårsak samt `command_id`, `correlation_id`, `operasjon_id`, `operasjonstype`, `attempt`, `timestamp`; ikke saksfeltene |

Bruk eksisterende saksfelter på kommandostatus. Operasjonshendelsen knyttes
til den med `command_id`. Ikke flett identifikatorer inn i `message`.
Manglende `correlation_id` eller klientreferanse utelates som før; ingen verdier
skal fabrikkeres eller hentes fra Sikris rå feilsvar. Test både sak adressert
med klientreferanse og sak adressert med arkivets saksnummer.

Begge DTO-er har `deny_unknown_fields`, som gjør at nye felt kan bryte gamle
konsumenter selv om feltene er valgfrie. Derfor endres ikke operasjons-DTO-en.
Hvis saksfeltene må finnes på hver enkelt operasjonshendelse, må det avtales
som en separat kontraktsendring og koordinert utrulling. Redigering av
`landdyrtilsyn-libs` utenfor dette repoet krever også separat tillatelse.

### 4.2 Bevar terminal årsak på kommandonivå

I `eksekver_operasjon.rs` skal den trygge meldingen og feilkoden fra den
terminalt feilende operasjonen videreformidles til kommandostatus. Bruk den
allerede typede feilen; ikke parse `siste_detalj` eller innfør Sikri-logikk i
application-laget. `hent_command_metadata` skal fortsatt leses etter commit,
slik at identifikatorene reflekterer oppdatert lokal tilstand.

Minste foreslåtte løsning uten ekstra persistens:

1. Gi `publiser_command_outcome` konteksten for hendelsen som utløser foldet:
   hendelse, trygg melding og eventuell feilkode.
2. Ved foldet `Feilet` og aktuell hendelse `Feilet`: publiser kommandoens
   terminale feil med denne årsaken. Flere feilende søsken kan gi flere
   terminale feilmeldinger, hver med sin reelle årsak.
3. Ved foldet `Feilet` etter at et søsken ble `Ok`: behold operasjonens
   success-hendelse, men ikke publiser en ny generisk kommandofeil som skyver
   bort den presise årsaken i klientens siste-status-visning.
4. Bevar foldets prioritet og behandling av `Fullfort` og `KreverAvklaring`.
   En kommando som har feilet må aldri publiseres som fullført.
5. La `Ugyldig` og `AlleredeUtfort` fortsatt gå gjennom den samme publiserende
   executor-stien. De skal ikke få nye stille terminale utfall.

Dette løser videreformidling under normal kjøring, ikke replay av historiske
feilmeldinger etter restart. Det eksisterer et commit/publish-gap: databasen
kan være terminal før JetStream har bekreftet hendelsen. Ikke påstå at denne
endringen gir exactly-once eller garantert levering. Feil i publisering skal
fortsatt være synlig og ikke svelges; en outbox er en separat arkitekturoppgave.

## 5. Avklart: terminale feil repareres senere gjennom admin-grensesnittet

`vurder_avslutt_sak` i `src/domain/src/eksekvering/operasjon.rs` krever i dag
at alle andre operasjoner på saken er `Ok`. En tidligere feilet `AvsluttSak`
blokkerer derfor en ny avslutning, selv om restansene er håndtert i arkivet.

Operatøren har bestemt at tidligere terminalt feilede forsøk skal repareres
gjennom admin-grensesnittet etter hvert. Denne leveransen skal derfor:

- Beholde dagens søskenkrav og terminale tilstander uendret.
- Ikke ignorere tidligere feilede avslutninger, gjenåpne operasjoner eller
  gjøre ny `command_id` til en reparasjonsmekanisme.
- Ikke be klienten sende `AvsluttSak` på nytt for å reparere en terminal feil.
- Publisere den presise årsaken og identifikatorene som trengs for å finne
  kommandoen ved senere administrativ oppfølging.

Admin-reparasjonens handlinger, autorisasjon, tilstandsoverganger og
statuspublisering planlegges separat. Det hevdes ikke at funksjonaliteten
finnes i dag, og den er ikke en forutsetning for utrulling av denne rettingen.

Planen [ferdigbehandling av journalpost](ferdigbehandle-journalpost.md) §11.3
foreslår et unntak fra søskenkravet og ny innsending som reparasjon. Det
forslaget skal ikke tas inn i denne leveransen; ved samordning av planene må
det erstattes med operatørens beslutning om senere admin-reparasjon.
Ingen revisjon av domenets søskenkrav eller tilhørende ADR er nødvendig her.

## 6. Tester og akseptansekriterier

### Klassifisering og HTTP-kallvei

- Syntetisk JSON med restansemelding og ulike positive antall gir R1-koden,
  irrecoverable og nøyaktig avtalt melding.
- Begge dokumentinnholdsformuleringer gir R3-koden og irrecoverable ved 500.
- Ukjent 500, tom body, ugyldig JSON, bare omtale av restanser og treff bare i
  ekkoet input gir ikke den nye R1-regelen. Test null/ugyldig antall også.
- Den nye vedleggsformuleringen bare i ekkoet input/stacktrace skal heller
  ikke gi et nytt R3-treff.
- 400, 401, 403, 429, 502, 503 og 404 beholder dagens atferd. Lås både
  klassifisering, intern kode og klientmelding for de nye tekstene ved 400:
  dagens dokumentinnholdsmapping har ulike statusgrenser for kode og
  klassifisering. Ikke endre denne eksisterende asymmetrien som sidearbeid.
- Utvid loopback HTTP-testene i `crates/sikri_client/src/api.rs` med body og
  test avslutnings- og vedleggskall gjennom `ensure_success`. Bruk injiserbar
  lokal send-hjelper etter eksisterende avskrivingsmønster; ingen ekte secrets.
- Adaptertester skal ta `SikriFeil::fra_http` gjennom domenemapping, ikke bare
  konstruere en ferdigklassifisert feil manuelt.
- Legg syntetiske sensitive markører i input/stack-felter og bevis at de ikke
  finnes i klientmeldinger, serialisert status eller `siste_detalj`.

### Application, repository og wire

- R1 og R3 lagres som `feilet`, ikke `retry_venter`, og er ikke kjørbare igjen.
- Operasjon og kommando får `feilet`, `terminal = true` og samme trygge årsak.
- En senere vellykket søskenoperasjon endrer ikke feilet kommando til fullført
  og publiserer ikke generisk erstatning for den presise feilmeldingen.
- Flere feilende søsken gir sanne, terminale hendelser med hver sin årsak.
- Test saksnummer, klientreferanse, command-/correlation-ID i serialisert
  kommandostatus; operation-ID, type og attempt i operasjonsstatus.
- Bevar utelatelse av manglende/ugyldige valgfrie identifikatorer.
- Bevar regresjonsdekning for dagens søskenkrav: tidligere feilet avslutning,
  feilet vedlegg og uavklart tidligere avslutning blokkerer fortsatt.

### Integrasjon med NATS og Postgres

Utvid `integration-tests/tests/command_sequence_e2e.rs` og teststøtten:

1. Vellykket oppsett av sak, deretter restansefeil kun på avslutning.
2. Vellykket oppsett av sak/journalpost, deretter innholdsfeil kun på vedlegg.
3. Les både command- og operation-subject fra JetStream og verifiser faktisk
   payload, ikke bare logglinjen «publisert», som skrives før publish-ack.
4. Bekreft at terminalt feilet operasjon ikke automatisk kjøres på nytt, selv
   om fake-arkivet senere ville akseptert kallet. Admin-reparasjon testes ikke
   som del av denne leveransen.

Dagens fake feiler alle kall ved feilmodus, og status-testhjelperen samler bare
command-eventer. Gjør feilinjeksjonen operasjonsspesifikk og legg til innsamling
av operation-eventer. Ikke bygg en generell arkivsimulator for denne rettingen.
Fake-baserte runtime-tester og lokale HTTP-tester er komplementær dekning,
ikke én ende-til-ende-test mot ekte Sikri.

Kjør før implementasjonen ferdigstilles:

```bash
cargo fmt --check
cargo test -p sikri_client
cargo test -p domain
cargo test -p application
cargo test -p infrastructure
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --exclude skuffen-integration-tests
cargo test -p skuffen-integration-tests
```

Integrasjonstestene krever tilgjengelig lokal container-runtime for NATS og
Postgres. Oppgi faktisk resultat og eventuelle miljøblokkere; ikke regn
ukjørte tester som bestått.

## 7. Leveranserekkefølge og dokumentasjon

1. Behold avgrensningen mot senere admin-reparasjon fra §5 og uendret
   wire-format fra §4. Krav om saksfelter også på operation-eventer krever
   egen avklaring.
2. Implementer R1/R3-regler og tester gjennom HTTP-/adaptergrensen.
3. Bevar årsak på kommandonivå og test eksisterende kontekstfelt på wire.
4. Verifiser at terminale tilstander og eksisterende søskenkrav er uendret.
5. Kjør runtime-testene og øvrige sjekker fra §6.
6. Oppdater `.agent/guides/observability.md` med ny kode, terminal semantikk,
   R3-varianten og skillet mellom logger og public status.
7. Oppdater relevant statusdokumentasjon i `README.md` og samordne §4.5/§11.3/
   §11.4 i planen for ferdigbehandling av journalpost med beslutningen om
   senere admin-reparasjon, ikke ny innsending som reparasjon. Merk bare
   ferdigstilt arbeid som levert. Kanoniske arkivregler skal ikke omskrives.

## 8. Produksjonskontroll og begrensninger

- Utrulling utføres bare etter særskilt bestilling. Ingen produksjonswrites,
  manuell retry eller endring av forfallstid inngår i planarbeidet.
- Eksisterende ventende operasjoner vurderes etter de nye reglene ved neste
  ordinære forsøk. Bare et nytt svar som faktisk matcher, gir terminal feil.
- Bekreft intern kode, terminal databasestatus og faktisk status i JetStream
  med riktig melding og identifikatorer. Logg om publiseringsstart alene er
  ikke bevis for levering eller klientkonsum.
- Bruk avtalte, sikre loggfelter; ikke aktiver bred debug-logging for kontrollen.
- Terminalisering reparerer verken saken eller vedleggsinnholdet. Et feilet
  vedlegg eller en tidligere feilet avslutning kan fortsatt blokkere
  avslutning. Reparasjon håndteres senere gjennom admin-grensesnittet og er
  ikke løst av denne planen.
- En rollback av koden gjenåpner ikke allerede terminale kommandoer. Ved
  feilklassifisering må videre håndtering besluttes av operatøren.
- Et krav om garantert statuslevering over commit/publish-gapet må avklares
  separat; denne planen endrer meldingsinnhold og klassifisering, ikke outbox.
