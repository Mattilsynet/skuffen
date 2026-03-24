---
trigger: always_on
description: Normative repo rules for coding agents in Skuffen. Gjelder alltid.
---

# Repo Rules

Normative regler for hvordan kode, dokumentasjon og agentarbeid skal se ut i Skuffen.

## Source of truth

1. `AGENTS.md` er startpunktet.
2. Denne filen er den normative regelkilden for repoet.
3. `.agent/guides/` inneholder teknisk veiledning.
4. `.agent/skills/arkivfag/` er kanonisk arkivfaglig domain knowledge og skal behandles som hovedkilde for archive-domain rules.

## Architecture and boundaries

1. **Streng lagdeling:**
   - Domain: business rules og invariants; ingen I/O.
   - Application: use cases, ports og orchestration.
   - Infrastructure: NATS/DB/HTTP adapters og external clients.
2. **Ingen lekkasje av concerns:** Transport, storage, tracing og DTO-mapping skal ikke lekke inn i Domain/Application.
3. **Functional core:** Hold domain logic deterministisk og side-effect free.
4. **Imperative shell:** Side effects skal ligge i infrastructure listeners, adapters og repositories.

## Coding Guidelines

1. **Idiomatisk Rust:** Følg offisielle Rust best practices. Bruk riktig error handling, borrowing og standard traits.
2. **Klar logikkseparasjon:** Hold kjerne-logikk adskilt fra tekniske detaljer. Domain/Application skal være uavhengig av eksterne interfaces.
3. **Modular arkitektur:** Streng separation of concerns.
   - Egen modul for infra/transport.
   - `http`, `nats` og andre eksterne interfaces håndterer kun kommunikasjon.
   - Domain/Application skal ikke ha I/O eller parsing. Ingen Serde eller tracing der.
4. **Security og supply chain:**
   - **Minimer attack surface:** Foretrekk stdlib eller enkel custom kode. Ta inn crates kun når det er nødvendig.
   - **Secrets:** Behandle tokens/keys som sensitive. Bruk `secrecy::Secret` for streng kontroll av debug og memory.
5. **Observability og safe logging:**
    - **Traceability:** Logging på tvers av lag (HTTP -> Domain -> DB). Bruk strukturert logging (f.eks. `tracing`) med correlation IDs/spans.
    - **Sanitization:** Ingen PII eller secrets i logs. Logg flyt og outcome, ikke sensitive payloads.
6. **Type safety:** Utnytt type system ("Parse, don't validate").
7. **DRY:** Gjenbruk kode der det gir mening. Hold kodebasen minimal.
8. **Comments:** Minimer kommentarer. Kun der det er nødvendig.

## Rust naming, imports, and modeling

1. **Rust naming:**
   - `snake_case` for modules, functions og variables.
   - `PascalCase` for types, enums og traits.
   - `SCREAMING_SNAKE_CASE` for constants.
2. **Imports:** Prefer explicit imports og hold grupper lesbare: std/core, third-party, lokale crate-imports.
3. **Value objects:** Foretrekk sterke typer over primitive felter nar invariants finnes.
4. **Validation:** Valider ved construction boundaries (`new`, `TryFrom`, `FromStr`).
5. **State:** Modellér state med enums, ikke string status fields.
6. **UUIDs:** Bruk UUIDs konsekvent for command ids, client references og interne ids.

## Error handling and async behavior

1. **Result-first:** Bruk `Result<T, E>` og propagér med `?`.
2. **Boundary errors:** Bruk `anyhow` ved orchestration boundaries og `thiserror` for gjenbrukbare typed errors.
3. **Context:** Legg til `anyhow::Context` rundt adapter- og service-feil.
4. **Failure semantics:** Bevar klassifiseringene `blocked`, `recoverable` og `irrecoverable`.
5. **No unwrap in production:** `unwrap()` er kun akseptabelt i tester eller ved beviste invariants.
6. **Tokio idioms:** Bruk `async fn`, `#[tokio::test]` og andre etablerte Tokio-mønstre konsekvent.
7. **Idempotency:** Design command ingestion og execution til a være idempotent.

## Norwenglish

1. **Svar:** Norsk, men behold English technical terms ("borrow checker", "trait", "query").
2. **Domain-navn:** Norsk for domain-spesifikke variabler/structs/enums/funksjoner.
   - Eksempel: `let antall_brukere = ...`, `fn beregn_skatt()`, `struct Saksbehandler`.
3. **Rust-konvensjoner:** English for standard Rust patterns/traits/keywords.
   - **Metoder:** `pub fn new()`, aldri `ny()`.
   - **Traits:** `From`, `Into`, `Default`, `Display`.
   - **Tekniske variabler:** `request`, `response`, `ctx`, `stream`.
4. **Kommentarer:** Skriv kommentarer på norsk.

## Secrets, variables, IDs

1. **Ingen hardkoding:** Ikke hardkod interne project IDs, bucket names, tokens, eller andre drifts- og miljøverdier i kode, tester eller dokumentasjon.
2. **Miljovariabler:** Alle slike verdier skal settes via env vars lokalt og via GitHub Actions secrets/vars i CI.
3. **Testverdier:** Bruk kun syntetiske placeholders i tester (f.eks. `example-project-id`). Aldri bruk ekte interne IDs.
4. **Dokumentasjon:** Bruk generiske eksempler (`<project-id>`, `your-project-id`), aldri reelle verdier.
5. **.env hygiene:** Lokale .env-filer skal ikke commits; hold dem i .gitignore og del eksempler som `.env.example` uten ekte verdier.
6. **Pre-commit sjekk:** Sjekk for utilsiktede verdier i diff/logg for sentrale env-navn/ID-er for hver PR.

## Testing and finalizing

1. **Async tests:** Bruk `#[tokio::test]` for async tester.
2. **Naming:** Navngi tester etter behavior og outcome.
3. **Structure:** Følg Arrange / Act / Assert.
4. **Test scope:** Foretrekk fakes og mocks i application-tester; behold end-to-end-atferd i integration tests.
5. **Before finalizing:** Kjor `cargo fmt` og relevante tester for endringen; kjor integration tests nar command flow eller infrastructure er berort.
