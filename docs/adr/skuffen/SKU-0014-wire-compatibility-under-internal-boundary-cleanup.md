# SKU-0014. Wire compatibility under internal boundary cleanup

Date: 2026-06-02
Last-reviewed: 2026-06-03
Tier: B
Status: Accepted
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
