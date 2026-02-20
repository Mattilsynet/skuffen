---
trigger: always_on
description: Rust coding guidelines og Norwenglish språkkonvensjoner. Gjelder alltid.
---

# Rust og språk

Regler for hvordan kode og kommunikasjon skal se ut i Skuffen.

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
