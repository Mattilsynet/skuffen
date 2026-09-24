# SKU-0003. SettSaksansvarlig — Noark 5 M306 i tilstandsmaskinen

Date: 2026-04-23
Last-reviewed: 2026-05-18
Tier: B
Status: Accepted
Crates: skuffen, domain, application, infrastructure

## Related

References: SKU-0016

## Context

Skuffen trengte `SettSaksansvarlig`-kommando for Sikri API. Spørsmålet: er saksansvarlig arkivkonsept (tilstandsmaskin) eller saksbehandlingskonsept (pass-through)? Noark 5 6.1.13 krever at saksansvarlig låses ved avslutting.

## Decision

R1 [5]: Saksansvarlig modelleres i tilstandsmaskinen med `oensket_saksansvarlig` og `naavaerende_saksansvarlig` som `Option<Saksansvarlig>` på `SakMedBarn`. `Saksansvarlig` value object har `saksbehandler_id` og `enhet`. Begge `None` når ingen forespurt.

R2 [5]: Noark 5 tjenestegrensesnitt v1.1 definerer saksansvarlig som M306 på Saksmappe. 6.1.13 krever at saksansvarlig ikke endres når Saksmappe avsluttes. 6.1.14 tillater andre metadata-endringer med logging. Skuffen følger arkivdomenet.

R3 [5]: Saksansvarlig er ikke prerequisite for journalpostarbeid. `planlegg_neste_handling` skal bare vurdere saksansvarlig som prerequisite for `AvsluttSak`, eller som direkte arbeid for `SettSaksansvarlig`.

R4 [5]: `AvsluttSak` blokkerer med eksplisitt `BlockedReason` med mindre `oensket_saksansvarlig == naavaerende_saksansvarlig`. Den skal ikke returnere `SettSaksansvarlig` eller utføre saksansvarligarbeid.

R5 [5]: `SettSaksansvarlig` er idempotent: hvis `oensket_saksansvarlig == naavaerende_saksansvarlig`, returnerer `planlegg_neste_handling` `CommandStateDecision::Done` for kommandoen.

R6 [5]: Migrasjon (`20260423120000_add_sett_saksansvarlig.up.sql`) legger til 4 kolonner på `sak_tilstand` og utvider CHECK-constraints på `command_execution`.

### Forkastet alternativ: Pass-through

Pass-through som omgår tilstandsmaskinen ble forkastet fordi:
- Noark 5-forskning beviste at saksansvarlig er arkivmetadata
- En sak kan ikke avsluttes uten korrekt saksansvarlig
- Pass-through ville ikke gi `AvsluttSak` en faktabasert prerequisite å blokkere på

### Observabilitet

Sikri-kallet logges med `saksnr` men **ikke** `saksbehandler_id` eller `enhet` — disse kan inneholde personidentifiserbare opplysninger (safe logging-prinsipp).

## Consequences

Tilstandsmaskinen har ikke lenger et steg 1b som setter saksansvarlig før journalpostarbeid. Kompleksiteten ligger i `AvsluttSak`-guard og `SettSaksansvarlig` sin idempotente command decision. `SakMedBarn`-konstruktører må inkludere saksansvarlig-facts. `Saksansvarlig` value object gir typesikkerhet i domain/infrastructure/tests.
