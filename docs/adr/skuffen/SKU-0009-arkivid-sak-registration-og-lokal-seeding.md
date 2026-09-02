# SKU-0009. ArkivId-sak: validering og lokal sak_tilstand-seeding

Date: 2026-05-26
Last-reviewed: 2026-09-02
Tier: B
Status: Accepted
Crates: skuffen, domain, application, infrastructure, sikri_client, skuffen-integration-tests

## Related

References: SKU-0016, SKU-0011

## Context

Skuffen kan motta kommandoer for Saker det aldri har sett lokalt. Ved `SakKey::ArkivId` finnes Saken allerede i Sikri, men lokal `sak_tilstand` mangler. Test-rollout skal derfor støtte trygg seeding etter arkivvalidering, uten production backfill.

## Decision

R1 [5]: Skuffen kan motta kommandoer for Saker det ikke har sett før. For `SakKey::ArkivId` må archive validation verifisere at Sak eksisterer og er åpen før lokal seeding.

R2 [5]: Validation er verifiserings-gate som sjekker arkiv. Registration materialiserer lokale facts etter at validert kommando ankommer. Validation skriver ikke state; registration gjør det.

R3 [5]: Når `SakKey::ArkivId` valideres mot arkiv/Sikri og Sak er åpen, skal registration seede lokal `sak_tilstand` som `SakTilstand::Opprettet` med `saksnummer`, selv om Skuffen aldri har behandlet `OpprettSak` for den.

R4 [5]: `IkkeRealisert` tilhører bare `OpprettSak`-commanden. ArkivId-seeded Saker er allerede realisert i arkiv og omgår `IkkeRealisert`-tilstanden.

R5 [5]: Seeding må være idempotent og må ikke overskrive eksisterende `tilstand`, `saksnummer` eller `opprettet_av_command_id` på `sak_tilstand`.

R6 [5]: `opprettet_av_command_id` på seeded row er lokal provenance: kommandoen som først materialiserte lokal fact, ikke kommandoen som opprettet Sak i arkiv.

R7 [5]: Stabil `skuffen_id` er grunnen til at Skuffen kan trygt bridge eksterne arkiv/client-identiteter. Seeding bruker denne som primær nøkkel.

R8 [5]: `OpprettSak` mot en `client_reference` som allerede har `arkiv_id` avvises i validering som irrecoverable. Saken finnes i arkivet, og en ny opprettelse ville gitt duplikat. Andre operasjoner mot eksisterende saker er fortsatt tillatt.

## Consequences

- ArkivId-kommandoer kan starte fra eksisterende åpne Saker i arkiv.
- Registration får ansvar for idempotent local fact-materialisering.
- `OpprettSak` beholder `IkkeRealisert`; ArkivId-seeding går direkte til `Opprettet`.
- Test-only rollout utsetter production-safe backfill/migration.
- R8 fanger kun gjenbruk av en `client_reference` Skuffen selv har arkivert.
  Sender klienten en ny referanse for en sak som finnes i arkivet under et annet
  saksnummer, kan Skuffen ikke vite det — payloaden bærer ikke saksnummer. Det er
  en grense i kontrakten, ikke i implementasjonen.
