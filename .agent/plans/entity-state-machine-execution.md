# Implementeringsplan: Entity State Machine Execution Model

**Opprettet:** 2026-04-20  
**Beslutningsdokument:** `.agent/decisions/entity-state-machine-execution.md`

## Bakgrunn og mål

Skuffens eksekveringssystem skal bygges om. Det nåværende systemet re-deriverer en in-memory plan (`EksekveringsPlan::fra_command()`) hver gang en kommando kjøres. Det finnes ingen per-steg persistering — du kan ikke se i databasen hvor en kommando er i sin eksekvering. Systemet skal erstattes med **persisterte tilstandsmaskiner per domeneentitet**.

### Ny modell i korte trekk

- Hver domeneentitet (sak, journalpost, dokument) får en egen rad i en per-type tilstandstabell
- Raden har `tilstand` (nåtilstand) og `ønsket_tilstand` (ønsket slutttilstand) — begge persistert i DB
- Rene domenefunksjoner avgjør neste overgang basert på nåværende tilstander (ingen IO)
- Executoren er en "gap-closer": last tilstander → spør domenet → utfør én Sikri-operasjon → oppdater tilstand → gjenta
- `command_execution` beholdes som scheduling/retry-autoritet (jobbkø)
- Entitetstabeller erstatter `sak_state`, `journalpost_state`, `dokument_state` (snapshot-tabeller)
- `tilstand_historikk`-tabell gir audit trail
- `Ventegrunn` fjernes — blokkering er implisitt (domenet returnerer `None` fra `neste_handling`)
- `venter` status i command_execution renames til `blokkert_venter`

### Viktige regler

- Feilet dokument blokkerer journalpost (og transitvt sak). `kan_journalfoere` returnerer `Blocked` hvis noe dokument har feilet. Dette er korrekt oppførsel.
- Journalpost har obligatorisk livssyklus: opprettet → dokumenter → journalført → (avskrevet for inngående). Klienten kan ikke stoppe midtveis.
- Sak har klientstyrt livssyklus: `OpprettSak` setter ønsket til `Opprettet`. `AvsluttSak` oppdaterer ønsket til `Avsluttet`. Kan være blokkert av ekstern tilstand.
- Sak uten barn er en vanlig case: `OpprettSak` → vent på saksnummer → klient bruker saksnummer i hoveddokument → `OpprettJournalpost`.
- Flere kommandoer deler samme sak-entitet.

### Forutsetninger

- Systemet er ikke i produksjon. Databasen kan slettes og re-opprettes.
- Alle migrasjoner erstattes med én ny base-migrasjon (clean slate).
- Ingen dual-write eller migreringsperiode nødvendig.

---

## Filer som skal endres eller fjernes

### Domain layer (`src/domain/src/eksekvering/`)

| Fil | Handling |
|---|---|
| `plan.rs` | **Fjernes.** Erstattes av tilstandsmaskin-typer og `neste_handling`-funksjoner. Valideringslogikken (`valider_felles`) extraheres til ny `validering.rs`. |
| `execution.rs` | **Betydelig endring.** `EksekveringStatus::Venter` → `BlokkertVenter`. `Ventegrunn` fjernes. `Kjorbarhet` og `Eksekveringsresultat` revideres. |
| `regler.rs` | **Utvides.** Blir hjertet i tilstandsmaskinen. Nåværende `kan_*`-funksjoner beholdes og utvides til å drive `neste_handling`. `SakRuleState`/`JournalpostRuleState` erstattes av nye tilstandsenums. |
| `typer.rs` | **Beholdes i hovedsak.** `EksekveringFeil`, lifecycle-typer overleverer. |
| `id.rs` | **Beholdes.** `SkuffenSakId`, `SkuffenJournalpostId`, `SkuffenDokumentId`. |
| `mod.rs` | **Oppdateres.** Nye moduler eksporteres. |
| *Ny:* `tilstand.rs` | **Ny fil.** Tilstandsenums per entity type, `SakMedBarn`-aggregat, `ArkivOperasjon`-enum, `ønsket_sluttilstand_for`-funksjoner. |
| *Ny:* `validering.rs` | **Ny fil.** `valider_kommando()` extrahert fra `plan.rs`. |

### Application layer (`src/application/src/command/`)

| Fil | Handling |
|---|---|
| `services/eksekver_kommando.rs` | **Skrives om.** Ny executor loop: last `SakMedBarn` → `neste_handling()` → utfør → oppdater tilstand → loop. |
| `services/eksekver_kommando/plan_resolver.rs` | **Fjernes.** |
| `services/eksekver_kommando/resolved_plan.rs` | **Fjernes.** |
| `services/eksekver_kommando/step_outcome.rs` | **Fjernes.** |
| `services/eksekver_kommando/sak_handlers.rs` | **Revideres.** Blir tynne handlers kalt av executor basert på `ArkivOperasjon`. |
| `services/eksekver_kommando/journalpost_handlers.rs` | **Revideres.** Samme. |
| `services/eksekver_kommando/dokument_handlers.rs` | **Revideres.** Samme. |
| `services/eksekver_kommando/wakeup.rs` | **Revideres/forenkles.** Wakeup trigger basert på entitetstilstandsendringer. |
| `services/eksekver_kommando/prerequisite.rs` | **Fjernes.** Prerequisites er implisitte i tilstandssjekker. |
| `services/eksekver_kommando/execution_report.rs` | **Revideres.** Rapport deriveres fra entitetstilstander. |
| `services/eksekver_kommando/lifecycle_publisher.rs` | **Revideres.** Lifecycle events deriveres fra entitetstilstander. |
| `services/eksekvering_worker.rs` | **Beholdes i hovedsak.** Poll loop og scheduling forblir. Outcome-mapping oppdateres. |
| `services/registrer_i_eksekveringssystem.rs` | **Skrives om.** Oppretter entitetsrader i tilstandstabeller ved registrering. |
| `services/eksekveringsklarhet_vurderer.rs` | **Fjernes/forenkles.** Klarhetsvurdering gjøres av domenet via tilstandssjekk. |
| `services/reevaluer_ventende_kommandoer.rs` | **Forenkles.** Wakeup: når entitetstilstand endres, marker `blokkert_venter`-kommandoer på samme sak som `klar`. |
| `ports/command_execution_port.rs` | **Revideres.** `Ventegrunn`-referanser fjernes. `venter` → `blokkert_venter`. Wait-kolonne-metoder fjernes. |
| `ports/execution_snapshot_port.rs` | **Fjernes.** Erstattes av ny entity tilstand port. |
| `ports/eksekvering_port.rs` | **Beholdes.** `ArkivGateway`, publishere. |
| `ports/execution_registration_port.rs` | **Revideres.** Registrering oppretter entitetsrader. |
| `ports/ventende_kommando_wakeup_port.rs` | **Forenkles.** |
| `ports/id_mapping_port.rs` | **Beholdes.** Separat livssyklus. |
| `ports/status_projection_port.rs` | **Beholdes.** |
| *Ny:* `ports/entity_tilstand_port.rs` | **Ny fil.** Port for per-type tilstandslesing/skriving, historikk, `SakMedBarn`-lasting. |

### Infrastructure layer (`src/infrastructure/src/command/`)

| Fil | Handling |
|---|---|
| `adapter/postgres_execution_store.rs` | **Betydelig reskriving.** Snapshot-metoder fjernes. Ny entity tilstand adapter. `venter` → `blokkert_venter` i alle queries. |
| `adapter/sikri_arkiv_gateway.rs` | **Beholdes.** Sikri API adapter er uendret. |
| `nats/eksekvering_listener.rs` | **Beholdes i hovedsak.** |

### Migrasjoner (`src/infrastructure/migrations/`)

| Handling |
|---|
| **Slett alle eksisterende migrasjoner.** Opprett én ny base-migrasjon med alt (id_mapping + nye tilstandstabeller + command_execution). Clean slate. |

### Integrasjonstester (`integration-tests/`)

| Handling |
|---|
| **Oppdater alle.** Tester som bruker snapshot-porter eller `Ventegrunn` må skrives om. |

---

## Nytt databaseskjema (base-migrasjon)

### Tabeller som beholdes (revidert)

**`id_mapping`** — beholdes som-er (inkl. nullable client_reference, command_id). Separat livssyklus.

**`command_execution`** — beholdes som jobbkø/scheduling. Endringer:
- `status` CHECK: `venter` → `blokkert_venter`
- Fjern `wait_kind`, `wait_sak_id`, `wait_journalpost_id` kolonnene og tilhørende CHECKs
- Fjern FK til `sak_state`/`journalpost_state` — erstatt med FK til nye tilstandstabeller
- Behold: `command_id`, `correlation_id`, `payload`, `command_type`, `sak_id`, `journalpost_id`, `status`, `attempt_no`, `retry_ready_at`, `last_detail`, `utfores_venter_publisert_at`, `created_at`, `updated_at`, `started_at`, `finished_at`

**`command_execution_attempt`** — beholdes. `outcome` CHECK: `venter` → `blokkert_venter`.

### Nye tabeller (erstatter snapshot-tabeller)

**`sak_tilstand`**
```sql
CREATE TABLE sak_tilstand (
    sak_id UUID PRIMARY KEY,
    tilstand VARCHAR(20) NOT NULL,
    ønsket_tilstand VARCHAR(20) NOT NULL,
    sikri_id BIGINT NULL,
    saksnummer VARCHAR(64) NULL,
    opprettet_av_command_id UUID NOT NULL,
    feil_detalj TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (tilstand IN ('ikke_realisert', 'opprettet', 'avsluttet', 'feilet_permanent')),
    CHECK (ønsket_tilstand IN ('opprettet', 'avsluttet')),
    CHECK (tilstand <> 'avsluttet' OR saksnummer IS NOT NULL),
    CHECK (tilstand <> 'feilet_permanent' OR feil_detalj IS NOT NULL)
);
```

**`journalpost_tilstand`**
```sql
CREATE TABLE journalpost_tilstand (
    journalpost_id UUID PRIMARY KEY,
    sak_id UUID NOT NULL REFERENCES sak_tilstand(sak_id),
    journalposttype VARCHAR(1) NOT NULL,
    med_utsending BOOLEAN NOT NULL DEFAULT false,
    tilstand VARCHAR(30) NOT NULL,
    ønsket_tilstand VARCHAR(30) NOT NULL,
    sikri_id BIGINT NULL,
    journalpostnummer INTEGER NULL,
    opprettet_av_command_id UUID NOT NULL,
    feil_detalj TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (journalposttype IN ('I', 'U', 'X')),
    CHECK (tilstand IN (
        'ikke_realisert', 'opprettet', 'dokumenter_under_arbeid',
        'klar_for_journalforing', 'venter_paa_utsending',
        'journalfoert', 'avskrevet', 'feilet_permanent'
    )),
    CHECK (ønsket_tilstand IN ('journalfoert', 'avskrevet')),
    CHECK (NOT med_utsending OR journalposttype = 'U'),
    CHECK (tilstand <> 'feilet_permanent' OR feil_detalj IS NOT NULL)
);
```

**`dokument_tilstand`**
```sql
CREATE TABLE dokument_tilstand (
    dokument_id UUID PRIMARY KEY,
    journalpost_id UUID NOT NULL REFERENCES journalpost_tilstand(journalpost_id),
    tilstand VARCHAR(20) NOT NULL,
    ønsket_tilstand VARCHAR(20) NOT NULL DEFAULT 'ok',
    opprettet_av_command_id UUID NOT NULL,
    feil_detalj TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (tilstand IN ('ikke_realisert', 'ok', 'feilet_permanent')),
    CHECK (ønsket_tilstand IN ('ok')),
    CHECK (tilstand <> 'feilet_permanent' OR feil_detalj IS NOT NULL)
);
```

**`tilstand_historikk`**
```sql
CREATE TABLE tilstand_historikk (
    id BIGSERIAL PRIMARY KEY,
    entity_type VARCHAR(20) NOT NULL,
    entity_id UUID NOT NULL,
    command_id UUID NOT NULL,
    fra_tilstand VARCHAR(30) NOT NULL,
    til_tilstand VARCHAR(30) NOT NULL,
    operasjon VARCHAR(64) NOT NULL,
    feil_detalj TEXT NULL,
    tidspunkt TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (entity_type IN ('sak', 'journalpost', 'dokument'))
);
```

### Indekser

```sql
CREATE INDEX ix_journalpost_tilstand_sak_id ON journalpost_tilstand(sak_id);
CREATE INDEX ix_dokument_tilstand_journalpost_id ON dokument_tilstand(journalpost_id);
CREATE INDEX ix_tilstand_historikk_entity ON tilstand_historikk(entity_type, entity_id);
CREATE INDEX ix_tilstand_historikk_command ON tilstand_historikk(command_id);
CREATE INDEX ix_command_execution_runnable ON command_execution(status, retry_ready_at, created_at);
CREATE INDEX ix_command_execution_sak ON command_execution(sak_id);
```

### Tabeller som fjernes
- `sak_state`
- `journalpost_state`
- `dokument_state`

---

## Nye domenetyper

### Tilstandsenums (i `domain/src/eksekvering/tilstand.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SakTilstand {
    IkkeRealisert,
    Opprettet,
    Avsluttet,
    FeiletPermanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalpostTilstand {
    IkkeRealisert,
    Opprettet,
    DokumenterUnderArbeid,
    KlarForJournalforing,
    VenterPaaUtsending,  // kun utgående med utsending
    Journalfoert,
    Avskrevet,           // kun inngående
    FeiletPermanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DokumentTilstand {
    IkkeRealisert,
    Ok,
    FeiletPermanent,
}
```

### `SakMedBarn`-aggregat

```rust
pub struct SakMedBarn {
    pub sak_id: SkuffenSakId,
    pub tilstand: SakTilstand,
    pub ønsket_tilstand: SakTilstand,
    pub sikri_id: Option<i64>,
    pub saksnummer: Option<String>,
    pub journalposter: Vec<JournalpostMedDokumenter>,
}

pub struct JournalpostMedDokumenter {
    pub journalpost_id: SkuffenJournalpostId,
    pub tilstand: JournalpostTilstand,
    pub ønsket_tilstand: JournalpostTilstand,
    pub sikri_id: Option<i64>,
    pub journalpostnummer: Option<i32>,
    pub journalposttype: JournalpostType,
    pub med_utsending: bool,
    pub dokumenter: Vec<DokumentMedTilstand>,
}

pub struct DokumentMedTilstand {
    pub dokument_id: SkuffenDokumentId,
    pub tilstand: DokumentTilstand,
}
```

Merk: `SakMedBarn` kan ha tom `journalposter`-vektor. En sak uten barn er en vanlig case (opprett sak → vent på saksnummer → klient sender journalpost-kommando senere).

### `ArkivOperasjon`-enum

```rust
pub enum ArkivOperasjon {
    OpprettSak { sak_id: SkuffenSakId },
    OpprettJournalpost { journalpost_id: SkuffenJournalpostId },
    LeggTilDokument { journalpost_id: SkuffenJournalpostId, dokument_id: SkuffenDokumentId },
    Journalfoer { journalpost_id: SkuffenJournalpostId },
    Avskriv { journalpost_id: SkuffenJournalpostId },
    AvsluttSak { sak_id: SkuffenSakId },
}
```

### Kjerne-funksjon: `neste_handling`

```rust
/// Gitt nåværende tilstander for en kommando sine entiteter,
/// returnerer neste Sikri-operasjon som kan utføres, eller None
/// hvis alt er ferdig eller blokkert.
pub fn neste_handling(
    command_type: CommandTypeCode,
    sak: &SakMedBarn,
) -> Result<Option<ArkivOperasjon>, EksekveringFeil>
```

Logikken:
1. Hvis sak er `IkkeRealisert` og ønsket >= `Opprettet` → `OpprettSak`
2. For hver journalpost som er `IkkeRealisert` og sak er `Opprettet` → `OpprettJournalpost`
3. For hvert dokument som er `IkkeRealisert` og journalpost er `Opprettet` → `LeggTilDokument`
4. Hvis alle dokumenter er `Ok` (ingen `FeiletPermanent`) og journalpost er `Opprettet`/`DokumenterUnderArbeid`/`KlarForJournalforing` → `Journalfoer`
5. Hvis journalpost er `Journalfoert` og ønsket er `Avskrevet` → `Avskriv`
6. Hvis sak ønsket er `Avsluttet` og alle journalposter i terminal tilstand → `AvsluttSak`
7. Hvis noe dokument er `FeiletPermanent` → returnerer `Err(EksekveringFeil::blocked(...))`
8. Ellers `None` (alt ferdig eller venter på noe utenfor denne kommandoen)

Funksjonen trenger bare data for **denne kommandoen sine entiteter**, ikke hele saken. For `AvsluttSak`-kommandoen trengs hele saken med alle journalposter.

### `ønsket_sluttilstand_for`-funksjoner

```rust
pub fn ønsket_sluttilstand_for_dokument() -> DokumentTilstand {
    DokumentTilstand::Ok
}

pub fn ønsket_sluttilstand_for_journalpost(
    journalposttype: JournalpostType,
) -> JournalpostTilstand {
    match journalposttype {
        JournalpostType::Inngaende => JournalpostTilstand::Avskrevet,
        JournalpostType::Utgaaende | JournalpostType::InterntNotat => JournalpostTilstand::Journalfoert,
    }
}

// Sak: ønsket settes eksplisitt av kommandoen, ikke av en funksjon
// OpprettSak → ønsket: Opprettet
// AvsluttSak → ønsket: Avsluttet (oppdaterer eksisterende rad)
```

---

## Faser

### Fase 0: Domain foundation
**Mål:** Bygg nye domenetyper og rene funksjoner. Ingen IO, ingen runtime-endringer.

**Arbeid:**
1. Opprett `src/domain/src/eksekvering/tilstand.rs`:
   - `SakTilstand`, `JournalpostTilstand`, `DokumentTilstand` enums
   - `SakMedBarn`, `JournalpostMedDokumenter`, `DokumentMedTilstand` aggregat-typer
   - `ArkivOperasjon` enum
   - `neste_handling()` funksjon
   - `ønsket_sluttilstand_for_*` funksjoner
2. Opprett `src/domain/src/eksekvering/validering.rs`:
   - Extraher `valider_kommando(command) -> Result<(), EksekveringFeil>` fra `EksekveringsPlan::fra_command`
   - Inkluder `valider_felles` og command-spesifikke valideringer (avsender for inngående, mottaker for utgående)
3. Rename `EksekveringStatus::Venter` → `BlokkertVenter` i `execution.rs`
4. Fjern `as_db_code()` fra domain-typer (flytt til infra)
5. Omfattende unit-tester for alle overgangsregler i `neste_handling`:
   - Sak uten barn: `IkkeRealisert` → `OpprettSak`
   - Journalpost med 3 dokumenter: korrekt rekkefølge
   - Feilet dokument blokkerer journalføring
   - Avskriving kun for inngående
   - `AvsluttSak` blokkert av uferdige journalposter
   - Sak allerede realisert → `neste_handling` returnerer `None` for OpprettSak-command
   - `VenterPaaUtsending` for utgående med utsending

**Gate:** `cargo test --workspace` passerer. Ingen runtime-endringer — gammel kode kompilerer fortsatt (kan ha ubrukte advarsler).

**Review etter fase:** Verifiser at `neste_handling` håndterer alle tilstandskombinasjoner, spesielt utgående med utsending. Exhaustive match-testing.

---

### Fase 1: Ny base-migrasjon + port + adapter
**Mål:** Ny database, nye porter og adaptere.

**Arbeid:**
1. Slett alle filer i `src/infrastructure/migrations/`
2. Opprett én ny base-migrasjon med alt:
   - `id_mapping` (som-er, inkl. alle endringer fra de tre gamle migreringene)
   - `sak_tilstand`, `journalpost_tilstand`, `dokument_tilstand` (nye)
   - `tilstand_historikk` (ny)
   - `command_execution` (revidert: `blokkert_venter`, uten wait-kolonner, FK til nye tabeller)
   - `command_execution_attempt` (revidert outcome CHECK)
   - Alle indekser
   - `set_updated_at()` trigger-funksjon
3. Opprett `src/application/src/command/ports/entity_tilstand_port.rs`:
   - `EntityTilstandRepository` trait med metoder for:
     - `opprett_sak_tilstand(...)`, `oppdater_sak_tilstand(...)`, `hent_sak_tilstand(...)`
     - Tilsvarende for journalpost og dokument
     - `hent_sak_med_barn(sak_id) -> SakMedBarn`
     - `hent_entiteter_for_kommando(command_id) -> ...` (for kommandoer som ikke har sak-barn-struktur)
     - `logg_overgang(...)` for tilstand_historikk
4. Implementer Postgres-adapter i infrastructure
5. Fjern `EksekveringSnapshotRepository` port og all snapshot-relatert kode

**Gate:** Migrasjon kjører uten feil. Adapter-enhets-/integrasjonstester for CRUD.

---

### Fase 2: Registrering oppretter entiteter
**Mål:** Når en kommando registreres, opprettes entitetsrader i tilstandstabeller.

**Arbeid:**
1. Skriv om `registrer_i_eksekveringssystem.rs`:
   - `OpprettSak`: opprett sak_tilstand-rad (tilstand: `ikke_realisert`, ønsket: `opprettet`)
   - `OpprettJournalpost*`: opprett journalpost_tilstand-rad + dokument_tilstand-rader. Link til eksisterende sak.
   - `AvsluttSak`: oppdater eksisterende sak_tilstand-rad (ønsket: `avsluttet`)
2. Fjern/forenkle `eksekveringsklarhet_vurderer.rs`:
   - Klarhet avgjøres av domenet: last entitetstilstander, kall `neste_handling`. Hvis `Some(...)` → klar. Hvis `None` og ikke alle ferdige → blokkert_venter. Hvis `Err(...)` → feil.
3. Fjern `execution_registration_port.rs` snapshot-avhengigheter
4. Oppdater lifecycle event publishing for registrering

**Gate:** Kommandoer registreres og entitetsrader opprettes korrekt. `command_execution` status satt riktig basert på tilstandsevaluering.

---

### Fase 3: Executor gap-closer
**Mål:** Kjerneendringen — executoren bruker entitetstilstander og domenefunksjoner.

**Arbeid:**
1. Skriv om `EksekverKommandoService::handle`:
   ```
   loop {
       let sak_med_barn = repo.hent_sak_med_barn(sak_id).await?;
       match domain::neste_handling(command_type, &sak_med_barn)? {
           Some(operasjon) => {
               let resultat = self.utfør_operasjon(envelope, operasjon).await;
               self.oppdater_tilstand(operasjon, resultat).await?;
               self.logg_historikk(operasjon, resultat, command_id).await?;
           }
           None => break, // Ferdig eller blokkert
       }
   }
   ```
2. Handler-moduler (`sak_handlers`, `journalpost_handlers`, `dokument_handlers`) revideres til å ta `ArkivOperasjon` og returnere tilstandsovergang
3. Etter loopen: evaluer om kommandoen er ferdig (alle entiteter ved ønsket), blokkert (prerequisite mangler), eller feilet
4. Wakeup-logikk: når en kommando fullfører og endrer entitetstilstand, re-evaluer `blokkert_venter`-kommandoer på samme sak
5. Oppdater lifecycle event publishing til å derivere fra entitetstilstander

**Gate:** Full end-to-end integrasjonstester. Alle kommandotyper fungerer. NATS lifecycle events uendret. Du kan spørre `sak_tilstand`/`journalpost_tilstand`/`dokument_tilstand` og se full status.

---

### Fase 4: Cleanup
**Mål:** Fjern all gammel kode.

**Arbeid:**
1. Fjern `plan.rs` (erstattet av `tilstand.rs` + `validering.rs`)
2. Fjern `resolved_plan.rs`, `plan_resolver.rs`, `step_outcome.rs`, `prerequisite.rs`
3. Fjern `Ventegrunn` fra `execution.rs`
4. Fjern `EksekveringSnapshotRepository` port (om ikke allerede gjort i fase 1)
5. Fjern ubrukte typer: `SakRuleState`, `JournalpostRuleState`, gammel `SakState`/`SakStatus`/`SakTransition`, etc.
6. Oppdater `regler.rs`: behold `kan_*`-funksjoner men gjør dem til wrappers rundt tilstandssjekker, eller integrer dem i `neste_handling`
7. Rydd imports og modulstruktur

**Gate:** `cargo clippy --all-targets --all-features` rent. `cargo test --workspace`. Ingen referanser til gammel snapshot-/plan-kode. Integrasjonstester passerer.

---

## Executor-flow etter omskriving

```
EksekveringWorker::run()
  │
  ├── poll command_execution for status = 'klar' OR 'retry_venter' (med backoff)
  │
  ├── marker_kjorer(command_id)
  │
  ├── EksekverKommandoService::handle(envelope, attempt)
  │     │
  │     ├── valider_kommando(command)?           // domain, pure
  │     │
  │     ├── repo.hent_sak_med_barn(sak_id)?     // load entity states from DB
  │     │
  │     ├── loop {
  │     │     neste_handling(command_type, &sak_med_barn)?  // domain, pure
  │     │     match result {
  │     │       Some(op) → utfør Sikri-kall → oppdater tilstand → logg historikk → reload sak_med_barn
  │     │       None → break
  │     │     }
  │     │   }
  │     │
  │     ├── evaluer_utfall(sak_med_barn)        // domain, pure: ok/blokkert/feilet
  │     │
  │     └── publiser lifecycle events
  │
  ├── marker command_execution status basert på utfall
  │
  └── wakeup: re-evaluer blokkert_venter kommandoer på samme sak
```

---

## Kommando-til-entiteter mapping ved registrering

| Kommando | Entiteter opprettet | ønsket_tilstand |
|---|---|---|
| `OpprettSak` | 1 × sak_tilstand | sak: `opprettet` |
| `OpprettInngåendeJournalpost` | 1 × journalpost_tilstand, N × dokument_tilstand | jp: `avskrevet`, dok: `ok` |
| `OpprettUtgåendeJournalpost` | 1 × journalpost_tilstand, N × dokument_tilstand | jp: `journalfoert`, dok: `ok` |
| `OpprettInterntNotatJournalpost` | 1 × journalpost_tilstand, N × dokument_tilstand | jp: `journalfoert`, dok: `ok` |
| `AvsluttSak` | (ingen nye — oppdaterer eksisterende sak_tilstand) | sak: `avsluttet` |

---

## Viktige hensyn

### Crash recovery
Vinduet mellom "Sikri-kall lyktes" og "tilstand persistert i DB" eksisterer. Executor må håndtere replay: hvis den restartes midt i en kommando, laster den entitetstilstander (som kan være utdaterte) og `neste_handling` returnerer en operasjon som allerede er utført i Sikri. Handlers må sjekke Sikri-tilstand og være idempotente, akkurat som i dag.

### Sak uten barn
En `OpprettSak`-kommando oppretter bare sak_tilstand. `SakMedBarn` har tom `journalposter`-vektor. `neste_handling` for denne kommandoen trenger bare sjekke sak-tilstand. Når saken er `Opprettet`, er kommandoen ferdig. Journalposter kommer fra separate kommandoer senere.

### Wakeup-mekanisme
Når executor fullfører en kommando som endrer sak-tilstand (f.eks. sak fra `IkkeRealisert` → `Opprettet`), må den trigge re-evaluering av `blokkert_venter`-kommandoer på samme sak. Disse markeres som `klar` slik at de plukkes opp av worker.
