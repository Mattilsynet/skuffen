# SKU-0005. HTML-template rendering og tilstandsmaskin

Date: 2026-05-13
Last-reviewed: 2026-05-21
Tier: B
Status: Accepted
Crates: skuffen, domain, application, infrastructure

## Related

References: SKU-0007, SKU-0004

## Context

SKU-0007 lar rene domenefunksjoner beregne `CommandStateDecision` fra command state og entity facts. HTML-template dokumenter må passe inn uten konkurrerende prerequisite-policy og uten I/O i domain.

`Felt::Saksnummer` gjør dokumentet blokkert for rendering til Sikri har gitt saken `saksnummer`. Dette uttrykkes som `BlockedReason`, ikke som implisitt `None` eller entity-lagret ønsket tilstand.

## Decision

R1 [5]: HTML-template dokumenter skal registreres som `DokumentTilstand::AvventerRendring`, mens bytes-dokumenter fortsatt starter i tilstanden som gjør dem klare for vanlig `LeggTilDokument`.

R2 [5]: Tilstandsmaskinen skal ha `ArkivOperasjon::RenderDokument` som journalpost-commandens neste handling når HTML-template hoveddokument-facts er klare før `OpprettJournalpost` og før journalføring. Skuffen oppretter normalt ikke journalpost uten hoveddokument.

R3 [5]: `RenderDokument`-regelen skal ligge før journalføring. Permanente kontraktsfeil skal bli command execution-diagnostikk og terminal command-feil, ikke entity-lagret desired-state progress.

R4 [5]: v1 readiness for `Felt::Saksnummer` skal kreve `sak.saksnummer.is_some()`, og fremtidige `Felt`-varianter må definere egne pure readiness-predikater.

R5 [5]: Domain skal bare skanne tokens, validere `felter` mot tokens og substituere verdier med pure funksjoner uten I/O, tracing eller storage-kunnskap.

R6 [5]: Intake skal validere `mal_referanse` i NATS object store på samme måte som andre media-referanser, fordi Skuffen eier media etter fullført upload.

R7 [5]: Mismatch mellom deklarerte `felter` og faktiske HTML-tokens er en irrecoverable kontraktsfeil som setter dokumentet til `FeiletPermanent`.

R8 [5]: Nye dokumentoverganger skal skrive `tilstand_historikk` med `command_id`, på samme måte som andre entity state-machine overganger.

R9 [5]: HTML-template dokumenter som ikke er hoveddokument støttes ikke i v1. Rendering er avgrenset til hoveddokument; rendered PDF brukes som Sikri hoveddokument. Et ikke-hoveddokument-HtmlTemplate i `AvventerRendring` etter `OpprettJournalpost` settes til `FeiletPermanent` og kommandoen feiler terminalt. `OpprettJournalpost` blokkeres ikke av dette dokumentet.

R10 [5]: `AvventerRendring` er render-operasjonens readiness-faktum. Hvis `rendered_dokument_referanse` allerede er lagret fra et avbrutt forsøk, skal `RenderDokument` fortsatt planlegges når feltene er klare, og application skal fullføre idempotent ved å sette dokumentet til `Ok` uten å hente mal, rendre eller lagre PDF på nytt.

## Consequences

Execution engine avgjør readiness uten HTML-hent, og SKU-0007 sin no-I/O boundary i domain bevares. Persistence lagrer `felter`, ny dokumenttilstand og `rendered_dokument_referanse`. Sikri-adapteren bruker rendered PDF som hoveddokument; original `mal_referanse` sendes ikke. Deployed smoke 2026-05-21 bekreftet full sekvens til `Ok` i testmiljøet.
