# Skuffen Architecture Guide

Denne guiden samler arkitekturressurser, command/query-referanser og diagrammer for Skuffen.

## Core documents

- Design guidelines: `.agent/guides/architecture/design_guidelines.md`
- Architecture principles: `.agent/guides/architecture/architecture_principles.md`
- ID mapping and idempotency: `.agent/guides/architecture/id_mapping_and_idempotency.md`

## Commands and queries

- Commands: `.agent/guides/architecture/command/commands.md`
- Queries: `.agent/guides/architecture/command/query.md`
- Query mapping CSV: `.agent/guides/architecture/command/query.csv`
- Command mapping SVG: `.agent/guides/architecture/command/Skuffen - Kommando mapping til Operasjon.svg`

## Media upload

Media bruker `lib-nats` sin session-baserte request/reply-protokoll med base subject
`arkiv.arkiver.media`: `begin` tildeler en receiver, chunks sendes til receiver-sessionen,
og `commit` lagrer objektet. Receiver-ID i subjectet hindrer at chunks fordeles mellom
overlappende Skuffen-revisjoner. Lagring er idempotent for samme UUID, stoerrelse og SHA-256;
samme UUID med annet innhold er en konflikt. NATS-serveren maa tillate minst 2 000 000 bytes
per melding, som er default chunk size i protokollen.

## Diagrams

- Application architecture SVG: `.agent/guides/architecture/Skuffen - Application Architecture.svg`
- Flow diagrams: `.agent/guides/architecture/flows/`
- State machines: `.agent/guides/architecture/state_machines/`
- Text interpretations: `.agent/assets/diagram_text/INDEX.md`

## Related guides

- Observability: `.agent/guides/observability.md`
- Repo rules: `.agent/rules/repo_rules.md`
- Canonical archive-domain knowledge: `.agent/skills/arkivfag/SKILL.md`
