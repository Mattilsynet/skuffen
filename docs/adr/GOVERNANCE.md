# ADR Governance

Dette dokumentet beskriver Skuffen-spesifikk prosess og judgment for ADR-arbeid. Mekaniske regler kommer fra den project-local `adr-fmt` binaryen og konfigurasjonen i `adr-fmt.toml`.

## Source of truth

- `cargo run -p adr-fmt -- --guidelines`: generated governance reference for invariant ADR rules, lifecycle, naming, relationships og template requirements.
- `docs/adr/adr-fmt.toml`: domain mapping, crate ownership og stale directory.
- `docs/adr/GOVERNANCE.md`: Skuffen-prosess, judgment og konvensjoner som ikke kan håndheves mekanisk.
- `.agent/skills/arkivfag/`: canonical archive-domain knowledge. Ikke rewrite eller reinterpret den her.

## Domains

### COM — Common Foundation

COM brukes for tverrgående beslutninger som gjelder på tvers av Skuffen crates. Eksempler er observability, safe logging, Rust coding conventions, NATS/event conventions og felles utviklingsworkflow. COM er et foundation domain og inkluderes alltid i `--context`.

### SKU — Skuffen

SKU brukes for Skuffen-spesifikke beslutninger om runtime behavior, domain/application boundaries, infrastructure adapters, Sikri integration, command flow, state management og integration tests. SKU dekker workspace-pakkene `skuffen`, `domain`, `application`, `infrastructure`, `sikri_client` og `skuffen-integration-tests`.

## Norwenglish

Behold English structural fields/headings som `adr-fmt` krever. Skriv prosjektspesifikk prose på norsk/Norwenglish, og behold crate names, Rust concepts og standard technical terms på engelsk.

## Development-only tooling

`adr-fmt` er documentation tooling only. Det skal ikke introduseres som runtime dependency, container runtime artifact eller application release dependency. Release/build commands for applikasjonen skal være app-targeted eller eksplisitt ekskludere docs tooling der det er relevant.

## Workflow

1. Velg domain: `COM` for tverrgående foundation decisions, `SKU` for Skuffen-spesifikke decisions.
2. Start fra `TEMPLATE.md` og plasser ADR-en i `common/` eller `skuffen/`.
3. Bruk `Root: OWN-ID` for første ADR i et decision tree, eller `References:`/`Supersedes:` når ADR-en bygger på eksisterende beslutninger.
4. Kjør `cargo run -p adr-fmt -- --guidelines` ved behov for regelreferanse.
5. Kjør relevante adr-fmt checks før endringen ferdigstilles.

## Stale ADRs

ADR-er med terminal status flyttes til `stale/` og får `## Retirement` etter de genererte adr-fmt-reglene. Ikke slett historikk bare fordi en beslutning er erstattet.
