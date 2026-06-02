# SKU-0012. Domenetyper og wire-typer kan dele korte navn

Date: 2026-06-01
Last-reviewed: 2026-06-01
Tier: B
Status: Accepted
Crates: skuffen, domain, application, infrastructure

## Related

References: SKU-0011, SKU-0008

## Context

Skuffen har interne domenetyper og eksterne wire-typer som kan beskrive samme
forretningsbegrep. Korte, presise typenavn skal ikke forbeholdes wire-kontrakten.
Kode viser eierskap med modulpath eller alias der navn deles mellom lag.

## Decision

R1 [5]: Domenetyper og wire-typer kan dele korte navn når begge navnene er naturlige i hvert sitt lag.

R2 [5]: Type-eierskap uttrykkes med modulpath; navn alene er ikke autoritativt for om en type er domain, wire eller adapter.

R3 [5]: Kode som bruker flere typer med samme korte navn skal bruke full path eller tydelige `as`-aliaser.

R4 [5]: Domain eier interne begreper i `domain::*`; wire-kontrakter eies av kontraktseiende moduler eller dedikerte schema libraries.

R5 [5]: Like korte navn betyr ikke like struktur eller versjon; forskjeller håndteres med eksplisitt boundary-mapping.

R6 [5]: Application og infrastructure oversetter mellom lagene og skal ikke innføre synonymnavn bare for å unngå like korte typenavn.

## Consequences

- Domenet kan bruke presise typenavn uten lag-prefikser.
- Imports og type-signaturer må vise hvilken modul som eier typen.
- Application og infrastructure aliaserer wire- og domain-typer når samme korte navn møtes lokalt.
