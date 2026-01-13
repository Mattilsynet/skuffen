# Project Context: Skuffen

`skuffen` is an archiving middleware service for Mattilsynet, abstracting the complexity of the external Sikri archive API. It provides a stable, asynchronous, message-based interface for internal systems to archive cases and journal records.

## Core Responsibilities
- Receives commands via NATS.
- Translates commands into archive operations.
- Executives operations against Sikri's archive API.
- Manages state and exposes status/results.
- Operates on a "Best Effort" basis with robust retry mechanisms.

## Architecture
Strict Hexagonal Architecture with CQRS:

1.  **Domain**
    *   Pure business logic (Case/JournalPost rules, state transitions).
    *   No external dependencies.
2.  **Application**
    *   Orchestration.
    *   Ports (Traits for repositories, queries, adapters).
3.  **Infrastructure**
    *   NATS (Communication).
    *   PostgreSQL (Persistence).
    *   Sikri Archive API (External System).
    *   Blob Storage.
    *   Idempotency & Queue Handling.

## Communication Pattern
*   **Protocol**: NATS (No JWT, specialized account for `skuffen`).
*   **Pattern**: Async Command/Response.
*   **Clients**: Publish commands, listen for responses.

## Key Concepts

### Data Model Hierarchy
1.  **Sequence**: A logical grouping of commands (e.g., "Create Case" + "Add Journal Post").
2.  **Command**: Instruction from client (Async, Idempotent).
    *   *Examples*: `OpprettSak`, `OpprettInngåendeJournalpost`.
    *   *Public Contract*.
3.  **Operation**: Concrete action against the Archive API (Sequential, 1:1 mapping to Sikri call).
    *   *Examples*: `OpprettJournalpost`, `Journalfør`.
    *   *Internal Implementation*.
4.  **Query**: Read-only synchronous call returning flat DTOs.
    *   *Examples*: `Hent sak`, `Hent status`.

### Identity Mapping
Detailed ID mapping handles decoupling between systems:
`client-reference` (Client System) <-> `skuffen-id` (Internal Stable ID) <-> `arkiv-id` (External Sikri ID)

### State Machine
*   **Sak (Case)**: `Under behandling` -> `Avsluttet`.
*   **Journalpost (Record)**:
    *   Incoming: `Opprettet` -> `Journalført` -> `Avskrevet`.
    *   Outgoing: `Opprettet` -> `Ferdigstilt` -> (`Sendt`) -> `Journalført` -> `Avskrevet`.

### Error Handling & Reliability
*   **Idempotency**: All commands have a unique ID (`kommandoId`) and are tracked in an idempotency log.
*   **Retries**: Recoverable errors are retried. Irrecoverable errors stop execution and notify the client.
*   **Resilience**: Designed to withstand Archive API downtime without affecting clients.

## Tech Stack References
*   **DTO Schemas**: `https://github.com/Mattilsynet/landdyrtilsyn-libs/tree/master/lib-schemas/src/Skuffen`

## Developer Guidelines & Detailed Architecture

### Coding Standards
You are an expert Rust developer building this service. When writing code, always:
- Use idiomatic Rust patterns (ownership, borrowing, lifetimes, Result/Option handling).
- Follow Rust naming conventions (snake_case for variables/functions/modules, CamelCase for types/enums).
- Use **Norwegian** for all comments and domain-specific logic (e.g., `avslutt_sak`, `SaksID`).
- Use **English technical terms** (fagspråk) for architectural patterns and standard types. For example: `Query`, `Command`, `Repository`, `Adapter`, `DTO`, `Service`, `Handler` etc. (e.g., `SakQueryService`, `ElementsSakRepository`).
- Structure code cleanly into modules with strict separation of concerns (domain, application, infrastructure).
- Handle errors correctly at each layer:
    - Use `?` for all error handling. Avoid `unwrap` and `panic`.
    - Distinguish sharply between **Domain Errors** (business rule violations) and **App Errors** (technical faults).
    - Adapters (infrastructure) are responsible for translating external errors (e.g., HTTP 503) to `AppFeil`.
- Avoid unnecessary clones.
- Provide unit tests for domain logic and integration tests for application services (using mock repositories).

### Hexagonal Architecture & CQRS
The service follows a strict hexagonal architecture with a CQRS split.

1.  **DOMAIN Layer**:
    - Pure, isolated core with no external dependencies.
    - Contains only `domene::Sak`, `domene::Journalpost`, etc., and business rules (`avslutt_sak`).
2.  **APPLICATION Layer**:
    - Defines *ports* (traits) for what it needs (e.g., `SakRepository`, `SakQueryService`).
    - Orchestrates logic.
3.  **INFRASTRUCTURE Layer**:
    - Implements the ports defined by the application layer.
    - Responsible for translating between public messages and the internal private domain model.

#### CQRS Pattern
-   **COMMANDS (Write)**:
    -   Uses a "rich" `domene::Sak` entity.
    -   All business logic (e.g., `avslutt_sak`) must be methods on domain entities.
    -   Application services (e.g., `ArkivCommandService`) only orchestrate (load, call domain method, save via UnitOfWork).
-   **QUERIES (Read)**:
    -   Must **never** load the rich domain entity.
    -   Uses a separate `SakQueryService` port to fetch flat DTOs (e.g., `SakVisning`) directly.

### Shared Crates & Contracts
-   Shared crates (e.g., `felles_meldinger`) define the **public contract** (Command messages, Query messages, DTOs).
-   Infrastructure is responsible for translating these into the internal domain model.

### Data Types & Command Structure
The system accepts commands which are translated into operations against the archive. The system maintains an internal queue of operations to ensure robustness (e.g., if the archive is down).

**Commands:**
```rust
pub enum Kommando {
    OpprettSak(OpprettSak),
    OpprettInngåendeJournalpost(OpprettInngåendeJurnalpost),
    OpprettUtgåendeJournalpost(OpprettUgåendeJurnalpost),
    OpprettInterntNotatJournalpost(OpprettInterntNotatJurnalpost),
    AvsluttSak, //TODO
}
```

**Workflow:**
Commands are translated to operations.
*Example*: `OpprettInngåendeJournalpost` translates to: `OpprettJournalpost`, `LeggTilVedlegg` (optional), `Avskriv`, `Journalfør`.

**Resilience:**
- Skuffen always accepts commands, even if the archive is down.
- Holds an internal queue of operations and their status.
- Only returns errors on irrecoverable failures.
