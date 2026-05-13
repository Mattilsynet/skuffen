# SKU-0005. HTML-template rendering og tilstandsmaskin

Date: 2026-05-13
Last-reviewed: 2026-05-13
Tier: B
Status: Proposed
Crates: skuffen, domain, application, infrastructure

## Related

References: SKU-0002, SKU-0004

## Context

SKU-0002 lar rene domenefunksjoner drive entity state machine. HTML-template dokumenter må passe inn uten konkurrerende prerequisite-policy og uten I/O i domain.

`Felt::Saksnummer` gjør dokumentet uklart for `LeggTilDokument` til Sikri har gitt saken `saksnummer`, samme readiness-mønster som SKU-0003.

## Decision

R1 [5]: HTML-template dokumenter skal registreres som `DokumentTilstand::AvventerRendring`, mens bytes-dokumenter fortsatt starter i tilstanden som gjør dem klare for vanlig `LeggTilDokument`.

R2 [5]: Tilstandsmaskinen skal ha `ArkivOperasjon::RenderDokument` for å close gapet mellom `AvventerRendring` og `Ok` før journalføring kan fullføres.

R3 [5]: `RenderDokument`-regelen skal ligge etter permanent-feil-propagation og før journalføring, slik at `FeiletPermanent` aldri maskeres av render-readiness.

R4 [5]: v1 readiness for `Felt::Saksnummer` skal kreve `sak.saksnummer.is_some()`, og fremtidige `Felt`-varianter må definere egne pure readiness-predikater.

R5 [5]: Domain skal bare skanne tokens, validere `felter` mot tokens og substituere verdier med pure funksjoner uten I/O, tracing eller storage-kunnskap.

R6 [5]: Intake skal validere `mal_referanse` i NATS object store på samme måte som andre media-referanser, fordi Skuffen eier media etter fullført upload.

R7 [5]: Mismatch mellom deklarerte `felter` og faktiske HTML-tokens er en irrecoverable kontraktsfeil som setter dokumentet til `FeiletPermanent`.

R8 [5]: Nye dokumentoverganger skal skrive `tilstand_historikk` med `command_id`, på samme måte som andre entity state-machine overganger.

## Consequences

Execution engine kan avgjøre readiness uten å hente HTML, og SKU-0002 sin no-I/O boundary i domain bevares.

Persistence må utvides med ny dokumenttilstand, deklarerte `felter`, og `rendered_dokument_referanse`. Token/felter mismatch feiler terminalt i stedet for å vente.
