# SKU-0003. SettSaksansvarlig — Noark 5 M306 i tilstandsmaskinen

Date: 2026-04-23
Last-reviewed: 2026-05-04
Tier: B
Status: Accepted
Crates: skuffen, domain, application, infrastructure

## Related

References: SKU-0002

## Context

Skuffen trengte `SettSaksansvarlig`-kommando for Sikri API. Spørsmålet: er saksansvarlig arkivkonsept (tilstandsmaskin) eller saksbehandlingskonsept (pass-through)? Noark 5 6.1.13 krever at saksansvarlig låses ved avslutting.

## Decision

R1 [5]: Saksansvarlig modelleres i tilstandsmaskinen med `oensket_saksansvarlig` og `naavaerende_saksansvarlig` som `Option<Saksansvarlig>` på `SakMedBarn`. `Saksansvarlig` value object har `saksbehandler_id` og `enhet`. Begge `None` når ingen forespurt.

R2 [5]: Noark 5 tjenestegrensesnitt v1.1 definerer saksansvarlig som M306 på Saksmappe. 6.1.13 krever at saksansvarlig ikke endres når Saksmappe avsluttes. 6.1.14 tillater andre metadata-endringer med logging. Skuffen følger arkivdomenet.

R3 [5]: Step 1b i `neste_handling()` plasseres rett etter `OpprettSak`, før journalpostarbeid. Steget krever `saksnummer.is_some()` for å unngå blocked-retry-syklus siden Sikri-kallet trenger saksnummer.

R4 [5]: `AvsluttSak`-vakt hindrer avslutting med mindre `oensket_saksansvarlig == naavaerende_saksansvarlig`. Hvis de ikke matcher, returnerer tilstandsmaskinen `SettSaksansvarlig` i stedet for `AvsluttSak`.

R5 [5]: `er_ferdig()` sjekker saksansvarlig-likhet. `SettSaksansvarlig` er idempotent: hvis `oensket == naavaerende` returnerer maskinen `None` → ingen operasjon → `er_ferdig` → success.

R6 [5]: Migrasjon (`20260423120000_add_sett_saksansvarlig.up.sql`) legger til 4 kolonner på `sak_tilstand` og utvider CHECK-constraints på `command_execution`.

### Forkastet alternativ: Pass-through

Pass-through som omgår tilstandsmaskinen ble forkastet fordi:
- Noark 5-forskning beviste at saksansvarlig er arkivmetadata
- En sak kan ikke avsluttes uten korrekt saksansvarlig
- Pass-through ville ikke fanges av `er_ferdig()`, og `AvsluttSak` ville ikke kunne vente

### Observabilitet

Sikri-kallet logges med `saksnr` men **ikke** `saksbehandler_id` eller `enhet` — disse kan inneholde personidentifiserbare opplysninger (safe logging-prinsipp).

## Consequences

Tilstandsmaskin blir mer kompleks (steg 1b, vakt på steg 8). `SakMedBarn`-konstruktører må inkludere nye felt. `Saksansvarlig` value object gir typesikkerhet i domain/infrastructure/tests. Migrasjon kreves for eksisterende databaser.
