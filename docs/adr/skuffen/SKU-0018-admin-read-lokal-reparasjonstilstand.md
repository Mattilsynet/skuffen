# SKU-0018. Admin read er lokal reparasjonstilstand

Date: 2026-08-27
Last-reviewed: 2026-08-27
Tier: B
Status: Accepted
Crates: skuffen, application, infrastructure, skuffen-integration-tests

## Related

References: SKU-0008, SKU-0016, SKU-0013

## Context

Status-streamen er den klientvendte lifecycle- og avvisningskanalen: den
forteller hva som har skjedd. Når noe må repareres, trenger en operatør noe
annet — den lokale tilstanden Skuffen faktisk kommer til å bruke dersom en
operasjon kjøres på nytt.

Den tilstanden finnes bare i PostgreSQL, og deler av den er bevisst skjult for
normale klienter: interne `skuffen_id`-er, operasjonsrader, materialiserte
attributter og provenance. Valideringsavvisning persisteres ikke lokalt
(SKU-0016), så fravær av operasjoner beviser ingenting om årsaken.

Denne leveransen er read-only. Admin write, som skal utføre avgrensede
reparasjoner, kommer senere og er ikke dekket her.

## Decision

R1 [5]: Admin read er to eksakte NATS core request-reply-subjects,
`arkiv.admin.read.command.hent` og `arkiv.admin.read.sak.hent`, med
`NatsResponse<T>` som svarramme (jf SKU-0008 R6). Ingen wildcard-subscription.

R2 [5]: Admin read viser autoritativ nåværende PostgreSQL-tilstand. Den leser
ikke status-streamen, kaller ikke arkivet eller object store, og skriver aldri.

R3 [5]: Commandens `utfall` er et snapshot-fold over nåværende operasjonsrader
med prioriteten `feilet` > `krever_avklaring` > `fullfort` > `uavklart`.
`krever_avklaring` har egen verdi og skjules ikke som `uavklart`.

R4 [5]: En command uten operasjoner er `uavklart`. Admin read utleder aldri
`avvist`; hvorfor en command ble avvist tilhører status-streamen.

R5 [5]: En kjent sak-identitet uten `sak_tilstand` er success med `fakta: null`,
ikke «Sak not found». Identitet mintes ved ingest, før validering.

R6 [5]: Responsene er permissive: lagrede koder og fritekst returneres som
strings uten å revalidere dem med command-side typer. Saksbehandler ved
opprettelse, ønsket og nåværende saksansvarlig og journalpostens saksbehandler
er separate felter og flates aldri ut.

R7 [5]: Requesten har obligatorisk `utfort_av`. Det er selvdeklarert
attribusjon, ikke autentisering. Tillitsmodellen er den eksisterende
NATS-tilgangen.

R8 [4]: `utfort_av` er det eneste menneskeidentifiserende feltet som er tillatt
i `info!`. Verdien trimmes, avvises ved blankhet, control characters eller mer
enn 128 bytes, logges én gang per request og lagres aldri. Rå `ArkivId` logges
ikke; attribusjonsloggen bruker bare key-typen.

R9 [5]: Feilsvarene er kontrakt: `Invalid request format`, `Command not found`,
`Sak not found`, `Response too large` og `Internal error`. Interne feil og
SQL-detaljer ekkoes aldri.

R10 [5]: Hvert oppslag leser fra én `REPEATABLE READ READ ONLY`-transaksjon, og
svaret serialiseres og måles mot NATS-grensen før publish. Et for stort svar gir
`Response too large`, aldri en trunkert eller delvis sak.

## Consequences

Operatører kan slå opp en feilende kommando og saken den gjelder uten
databasetilgang, og se nøyaktig den tilstanden en reparasjon må forholde seg
til. Interne id-er blir synlige utad; det er bevisst, fordi de er nødvendige for
å adressere riktig mål.

Kontrakten binder oss til at `utfall` er et fold, ikke en kolonne. Nye
operasjonsstatuser må derfor vurderes mot foldereglene i R3.

Uten paginering er responsstørrelse en reell grense. Guarden gjør grensen
synlig som en stabil feil i stedet for en timeout, men et senere behov må løses
med et nytt målrettet eller paginert subject — ikke ved å endre denne
responsen stille.

`utfort_av` gir sporbarhet, ikke bevis. Den er selvdeklarert og er derfor ikke
en autentisert audit-logg. Sterkere garantier krever en egen beslutning, og bør
tas før admin write åpner for skriveoperasjoner.
