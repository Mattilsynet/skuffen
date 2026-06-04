# SKU-0013. Domain og application importerer ikke wire-typer

Date: 2026-06-02
Last-reviewed: 2026-06-02
Tier: A
Status: Accepted
Crates: domain, application, infrastructure

## Related

References: SKU-0012, SKU-0011, SKU-0007

## Context

Skuffen har eksterne wire-kontrakter og interne typer for command execution,
tilstandsmaskin, lifecycle og archive-domain regler. SKU-0012 avklarer at slike
typer kan dele korte navn. Den gir ikke wire-kontraktene eierskap over
application- eller domain-API. Import av schema crates i indre lag gjør
hexagonens dependency direction uklar.

## Decision

R1 [4]: `domain` og `application` skal ikke importere eksterne wire-typer fra schema-, NATS- eller adapter-crates.

R2 [4]: Infrastructure oversetter wire payloads, NATS subjects og ekstern serialisering til interne typer før application use cases kalles.

R3 [4]: Application-porter skal eksponere interne application/domain-typer, ikke `lib_schemas` envelopes, commands, DTO-er eller statuskoder.

R4 [4]: Domain eier rene command-, state-machine-, lifecycle- og value-object-typer uten avhengighet til kontraktseiende schema libraries.

## Consequences

- `domain` og `application` får compile-time boundary mot wire contracts.
- Infrastructure eier wire compatibility, NATS subject construction og mapping.
- Interne API-er må uttrykke application/domain behov, ikke speile wire schema.
- Wire compatibility og migration sequencing håndteres av egne contract decisions.
