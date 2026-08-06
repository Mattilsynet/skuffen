# SKU-0014. Wire compatibility under internal boundary cleanup

Date: 2026-06-02
Last-reviewed: 2026-08-05
Tier: B
Status: Superseded by SKU-0015
Crates: infrastructure, application, skuffen, skuffen-integration-tests

## Related

References: SKU-0013, SKU-0008, SKU-0011, SKU-0012

## Context

SKU-0013 flytter wire-type eierskap ut av domain og application. Den cleanupen
må ikke bli en skjult wire migration. SKU-0008 eier command intake/query wire
contract. `command_execution.payload` lagrer wire command JSON, og done-streamen
abonnerer `arkiv.command.done.>`.

## Decision

R1 [5]: Done-publisering skal fortsatt bruke `arkiv.command.done.<entity>.<commandId>` med subject segmentene `sak` og `journalpost`.

R2 [5]: `command_execution.payload` skal fortsatt lagre wire command JSON; command id, correlation id og idempotency semantics skal ikke endres.

R3 [5]: Status- og error-values eksponert via wire contracts, inkludert SKU-0008 R4 error replies, skal bevares value-kompatibelt.

R4 [5]: Subject segmentene `sak` og `journalpost` er wire routing tokens, ikke bevis på intern lifecycle entity-type.

R5 [5]: Cleanup PRs som erstatter wire-typer internt blokkeres inntil tests pinner done subjects, persisted payload og status/error mapping.

R6 [5]: Status/error mapping eies av infrastructure projection/publisher adapters når `lib_schemas` fjernes fra indre lag.

R7 [5]: `MappingEntityType` er application-lag persistens- og id-mapping-vokabular; `as_code()`-verdiene er DB/historikk-koder, ikke NATS routing tokens.

R8 [5]: Application-laget skal ikke bruke `MappingEntityType` eller andre application-typer til å konstruere `arkiv.command.*` subjects; infrastructure eier `{sak|journalpost}` token-mapping og all subject-konstruksjon.

## Consequences

- Infrastructure eier compatibility-preserving mapping mellom interne modeller og
  eksterne contracts; lekkasje av `MappingEntityType` inn i subject-konstruksjon
  er et boundary-brudd.
- Intern opprydding krever ikke coordinated consumer migration så lenge disse
  invariants holder.
- Endring av NATS subject, payload shape eller status/error values krever egen
  contract migration.

## Retirement

SKU-0014 er superseded by SKU-0015. Superseringen er avgrenset, ikke total:

- R5 (gaten som blokkerer wire-type-cleanup inntil pinning-tester finnes) oppheves.
  Skjermingssikker kontrakt-redesign er en koordinert breaking change; det finnes
  ingen live klienter, og det er derfor ikke lenger noe krav om at cleanup PRs skal
  blokkeres inntil tester pinner done subjects, persistert payload og status/error
  mapping value-kompatibelt.
- R2 (persistert `command_execution.payload` wire-JSON skal bevares kompatibelt)
  oppheves. Kun dev/test-data finnes, og payload wipes ved cutover, så det er ingen
  historisk payload-JSON som må deserialiseres på tvers av den nye shapen.

Alt annet i SKU-0014 gjelder fortsatt i ånd (subject-routing, infrastructure-eid
mapping, `MappingEntityType` som ikke-routing-vokabular).

Viktig nyanse: Boundary-prinsippet om at wire-typer IKKE skal blø inn i
application/domain eies av SKU-0013 og står HELT urørt av denne superseringen.
SKU-0013 R1-R4 gjelder fullt ut: `domain` og `application` importerer ikke
wire-typer, infrastructure oversetter ved laggrensen. Skjermingsredesignen tar
INGEN snarveier her; den nye wire-shapen (Tilgjengelighet, Korrespondansepart,
egne permissive respons-typer) lever i kontrakts-/infrastructure-laget og mappes
til interne typer som før.
