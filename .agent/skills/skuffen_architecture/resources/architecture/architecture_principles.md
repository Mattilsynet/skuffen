# Architecture Principles

The project adheres to a **Hexagonal Architecture** with **CQRS**, implemented via a workspace of crates and modules.

## Core Principles

### 1. Functional Core, Imperative Shell
   - The **Domain Layer** (and partly the Application Layer) encapsulates all business rules and should be as large as possible. It is pure Rust and handles **no I/O**.
   - The **Infrastructure Layer** should be as small as possible. It handles I/O, external dependencies, error handling, logging, and mapping to/from external data models (DTOs). It should take as few distinct "decisions" as possible.

### 2. Separation of Concerns & Layering

| Path | Layer | Role | Key Components |
| :--- | :--- | :--- | :--- |
| `src/domain/` | **Domain** | Core Business Logic | Entities (`Sak`, `Journalpost`), Value Objects, Operations (`journalfør.rs`). **Pure Rust, no I/O.** |
| `src/application/` | **Application** | Use Cases & Plugins | Ports (`SakPort`), Services (`HentSakService`). Orchestrates Domain <-> Infra. |
| `src/infrastructure/` | **Infrastructure** | Concrete Impls | NATS Listeners (`listener.rs`), DB adapters, Repositories (`SikriRepository`). |
| `crates/sikri_client/` | **Client** | External Client | Low-level HTTP client for Sikri API (`api.rs`). |

### 3. Observability & Replayability (NATS JetStream)
**All state changes and operations shall be stored on a JetStream on NATS as JSON.**
- This serves as the source of truth for all changes in the system.
- Enables better debugging and replaying of events.
- General Guideline: If something enters the domain/application layers and causes a change, that change explicitly be reflected in a JetStream stream.

## Detailed Layer Description

### Domain Layer (`src/domain/src/model`)
Encapsulates all business rules.

#### Core Entities
*   **`Sak` (Case)**
    *   **Identity**: `SakKey`. A composite key containing:
        *   `skuffen_id`: UUID (Internal stable ID).
        *   `arkiv_id`: `Saksnummer` (e.g., "2025/12345") (External Sikri ID).
    *   **Properties**:
        *   `sakstittel`: `Sakstittel` value object. Enforces max length 256. Supports `.uo_tittel()` for masking sensitive titles ("*****").
        *   `ordningsverdi`: Validates format (digits & max one hyphen).
        *   `saksstatus`: Enum (`UnderBehandling`, `Ferdig`, `Avsluttet`).
        *   `journalposter`: List of `Journalpost` entities.
*   **`Journalpost` (Record)**
    *   **Identity**: `JournalpostKey` (Can be `SkuffenId(Uuid)` or `ArkivId(String)`).
    *   **Types**: `Inngående`, `Utgående`, `InterntNotat`.
    *   **Status**: `Registrert` → `Reservert` → `Midlertidig` → `Ferdig` → `Ekspedert` → `Journalført`.

#### Operations
Atomic actions aimed at the archive (e.g., `journalfør`, `avskriv`, `legg_til_vedlegg`, `opprett_sak`).
Located in `src/domain/src/model/operasjon/`.

### Application Layer (`src/application/src/ports`)
Defines the "Shape" of the application via Ports (Traits) and Use Cases.

### Infrastructure Layer (`src/infrastructure/`)
Connects the application to the outside world.

#### 1. NATS Integration (`src/infrastructure/src/nats/`)
*   **Protocol**: JSON over NATS.

#### 2. Sikri Client (`crates/sikri_client/`)
*   **Capabilities** (`api.rs`):
    *   `get_sak(...)`: Fetches case details.
    *   `create_sak(...)`: Creates a new archive case.
    *   `alive()`: Health check endpoint.
*   **Data Models**: Uses internal DTOs (`ElementsSak`, `ElementsSakResponse`) defined in `dto/`.
