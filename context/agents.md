# Project Context: Skuffen

`skuffen` is a **middleware archiving service** designed to provide a stable, asynchronous interface on top of the complex and potentially unstable Sikri Archive API.

## 🏗 Architecture & Code Organization
The project adheres to a **Strict Hexagonal Architecture** with **CQRS**, implemented via a workspace of crates and modules.

### High-Level Directory Map
| Path | Layer | Role | Key Components |
| :--- | :--- | :--- | :--- |
| `src/domain/` | **Domain** | Core Business Logic | Entities (`Sak`, `Journalpost`), Value Objects, Operations (`journalfør.rs`). **Pure Rust, no I/O.** |
| `src/application/` | **Application** | Use Cases & PLugins | Ports (`SakPort`), Services (`HentSakService`). Orchestrates Domain <-> Infra. |
| `src/infrastructure/` | **Infrastructure** | Concrete Impls | NATS Listeners (`listener.rs`), DB adapters, Repositories (`SikriRepository`). |
| `crates/sikri_client/` | **Adapter** | External Client | Low-level HTTP client for Sikri API (`api.rs`). |

---

## 🧠 Domain Layer Deep Dive
The domain layer (`src/domain/src/model`) encapsulates all business rules.

### Core Entities
#### 1. `Sak` (Case)
*   **Identity**: `SakKey`. A composite key containing:
    *   `skuffen_id`: UUID (Internal stable ID).
    *   `arkiv_id`: `Saksnummer` (e.g., "2025/12345") (External Sikri ID).
*   **Properties**:
    *   `sakstittel`: `Sakstittel` value object. Enforces max length 256. Supports `.uo_tittel()` for masking sensitive titles ("*****").
    *   `ordningsverdi`: Validates format (digits & max one hyphen).
    *   `saksstatus`: Enum (`UnderBehandling`, `Ferdig`, `Avsluttet`).
    *   `journalposter`: List of `Journalpost` entities.

#### 2. `Journalpost` (Record)
*   **Identity**: `JournalpostKey` (Can be `SkuffenId(Uuid)` or `ArkivId(String)`).
*   **Types**: `Inngående`, `Utgående`, `InterntNotat`.
*   **Status**: `Registrert` → `Reservert` → `Midlertidig` → `Ferdig` → `Ekspedert` → `Journalført`.

### Operations (Planned)
Located in `src/domain/src/model/operasjon/`. These represent atomic actions aimed at the archive:
*   `journalfør`, `avskriv`, `legg_til_vedlegg`, `opprett_sak`.
*   *Note*: These files are currently largely empty/skeletal.

---

## 🧩 Application Layer Interfaces
Defines the "Shape" of the application (`src/application/src/ports`).

### Key Ports (Traits)
*   **`SakPort`**:
    *   `async fn hent(&self, sak_key: SakKey) -> Result<Sak>`
    *   `async fn opprett()` (**TODO**)
    *   `async fn avslutt()` (**TODO**)
*   **`QueryUseCase`**: Generic trait for read operations.
*   **Specific Use Cases**: `HentSakUseCase`, `HentJournalpostUseCase`.

### Active Services
*   **`HentSakService`**: Implements `HentSakUseCase`. Orchestrates fetching a case from the repository.

---

## ⚙️ Infrastructure & Integrations
Connects the application to the outside world.

### 1. NATS Integration (`src/infrastructure/src/nats/`)
*   **Protocol**: JSON over NATS.
*   **Listener**: `NatsReplier<Req, Res>`
    *   Generic generic wrapper that connects a NATS subject (e.g., "sak.hent") to a `UseCase`.
    *   Handles deserialization, error wrapping, and reply publishing.
    *   *Setup*: In `main.rs`, `setup_nats()` connects only. Listeners are manually instigated.

### 2. Sikri Client (`crates/sikri_client/`)
*   **Authentication**:
    *   Uses **GCP Secrets** (`sikri-api-cloud-username/password`).
    *   Basic Auth against `BASE_URL_SIKRI`.
*   **Capabilities** (`api.rs`):
    *   `get_sak(...)`: Fetches case details.
    *   `create_sak(...)`: Creates a new archive case.
    *   `alive()`: Health check endpoint.
*   **Data Models**: Uses internal DTOs (`ElementsSak`, `ElementsSakResponse`) defined in `dto/`.

---

## 🚦 Current Implementation Status
**As of Jan 2026, the project is in early development.**

| Component | Status | Details |
| :--- | :--- | :--- |
| **Read/Query** | 🟡 Partial | `hent_sak` is implemented and wired up. `hent_journalpost` is commented out. |
| **Write/Command** | 🔴 Skeletal | Command ports are defined but empty. No active NATS listeners for commands. |
| **Domain Logic** | 🟡 Defined | Entities exist, but detailed operation logic is missing (`journalfør.rs` is empty). |
| **Infrastructure** | 🟢 Ready | NATS connection and Sikri Client plumbing are in place. |

## 🗺 Roadmap / Missing Pieces
1.  **Implement Write Path**:
    *   Define `CommandUseCase` trait.
    *   Implement `OpprettSakService` in Application layer.
    *   Implement `SakPort::opprett` in Infrastructure using `sikri_client::create_sak`.
2.  **Enable Operations**:
    *   Flesh out logic in `src/domain/src/model/operasjon/*.rs`.
3.  **Idempotency**:
    *   Implement the "Idempotency Log" mentioned in README (currently missing in code).
4.  **Resilience**:
    *   Implement the internal operation queue for "Best Effort" availability.
