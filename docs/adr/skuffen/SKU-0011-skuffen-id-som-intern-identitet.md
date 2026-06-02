# SKU-0011. SkuffenId som intern identitet

Date: 2026-06-01
Last-reviewed: 2026-06-01
Tier: A
Status: Accepted
Crates: skuffen, domain, application, infrastructure, sikri_client, skuffen-integration-tests

## Related

References: SKU-0008, SKU-0009

## Context

Skuffen kobler eksterne referanser fra klienter og arkiv til lokal entity state,
execution-state og provenance. Eksterne referanser har ulike eiere, formater og
livsløp. Domain- og execution-modellen trenger derfor en stabil intern identitet
som er uavhengig av wire-kontrakter og arkivets referanseformer.

## Decision

R1 [4]: `SkuffenSakId`, `SkuffenJournalpostId` og `SkuffenDokumentId` er Skuffens interne identitet for Saker, Journalposter og Dokumenter.

R2 [4]: SkuffenId-verdier er stabile etter tildeling og endres ikke når eksterne referanser legges til, endres eller oversettes.

R3 [4]: Entity facts, command execution og domain-regler refererer til Skuffen-entiteter med SkuffenId-newtypes, ikke rå `Uuid`.

R4 [4]: `client_reference`, `ArkivId`, `saksnummer` og andre eksterne referanser brukes ved boundary, lookup, seeding, id-mapping og projection.

R5 [4]: Mapping mellom eksterne referanser og SkuffenId skjer ved eksplisitte boundaries før facts lagres og domain-regler vurderer entity facts.

## Consequences

- SkuffenId er den varige interne identiteten gjennom entity state, execution og re-evaluering.
- Eksterne referanser kan bevares for idempotency, lookup, outward status og audit uten å bli domain identity.
- Nye domain-API-er skal bruke SkuffenId-newtypes når de peker på Skuffen-entiteter.
- Eksisterende rå-`Uuid`-API-er som representerer Skuffen-entiteter er overgangsgjeld.
- Adaptere og repositories må gjøre identitetsoversetting synlig ved laggrensen.
