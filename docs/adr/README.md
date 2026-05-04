# Architecture Decision Records

Dette directory inneholder Architecture Decision Records (ADR) for Skuffen. ADR-er forklarer hvorfor viktige arkitekturvalg finnes, hvilke tradeoffs som ble akseptert, og hvilke regler fremtidige endringer skal følge.

## Tooling

Skuffen bruker en project-local vendored copy av `adr-fmt` i `crates/adr-fmt/`. Verktøyet er development/documentation tooling only og skal ikke legges til som application runtime dependency eller release artifact.

Kjør ADR-verktøy med:

```bash
cargo run -p adr-fmt -- --guidelines
cargo run -p adr-fmt -- --tree
cargo run -p adr-fmt -- --context skuffen
```

`--guidelines` er den genererte referansen for mekaniske ADR-regler. `--tree` viser domain tree. `--context <crate>` viser accepted decision rules som gjelder for et crate.

## Domains

Skuffen har to minimale ADR-domener:

- `COM` i `common/`: foundation domain for tverrgående konvensjoner og felleseiergods. COM inkluderes i alle `--context` queries.
- `SKU` i `skuffen/`: Skuffen-spesifikke beslutninger for `skuffen`, `domain`, `application`, `infrastructure`, `sikri_client` og `skuffen-integration-tests`.

Terminale eller utdaterte ADR-er flyttes til `stale/` etter adr-fmt-reglene.

## Norwenglish convention

ADR-er bruker Norwenglish:

- English structural fields and headings required by `adr-fmt` stay English, for example `Date`, `Last-reviewed`, `Tier`, `Status`, `## Related`, `## Context`, `## Decision` and `## Consequences`.
- Project-specific prose, begrunnelser og domain language skrives på norsk/Norwenglish.
- Crate names, Rust terms og etablerte technical terms beholdes på engelsk.

## Migrated decisions

Tidligere legacy ADR og `.agent/decisions/`-notater er migrert til `docs/adr/skuffen/`.
Nye beslutninger skal bruke `COM-NNNN` eller `SKU-NNNN` naming og ligge i riktig domain directory.
