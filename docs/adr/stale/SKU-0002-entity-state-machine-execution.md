# SKU-0002. Entity State Machine Execution Model

Date: 2026-04-20
Last-reviewed: 2026-05-18
Tier: A
Status: Superseded by SKU-0007
Crates: skuffen, domain, application, infrastructure

## Related

References: SKU-0001

## Context

Det eksisterende eksekveringssystemet re-deriverer en in-memory plan hver gang en kommando kjøres. Det finnes ingen per-steg persistering — man kan ikke se i databasen hvor en kommando er i sin eksekvering. Systemet skal bygges om for debuggbarhet, sikkerhet, og partial-success-støtte.

Nåværende tilnærming med ephemeral plan + snapshot-tabeller gir:
- Ingen visibilitet i runtime state utenfor kommandokøen
- Vanskelig å håndtere partial success når steg mislykkes midt i en plan
- Manglende audit trail for tilstandsoverganger på entitetsnivå
- Feilbar re-deriving av plan ved hver kommando-eksekvering

## Decision

R1 [5]: Erstatt ephemeral plan + snapshot-tabeller med persisterte tilstandsmaskiner per domeneentitet (sak, journalpost, dokument).

R2 [5]: Bruk per-type tilstandstabeller (`sak_tilstand`, `journalpost_tilstand`, `dokument_tilstand`) fremfor én generisk `entity_tilstand`-tabell. Entitetstypene har fundamentalt ulike tilstandsformer, og Postgres CHECK constraints + Rust enums gir reell typesikkerhet.

R3 [5]: Hver entitetsrad skal ha både `tilstand` (nåtilstand/fakta) og `ønsket_tilstand` (ønsket slutt/intensjon). Executoren closer gapet mellom disse.

R4 [5]: `oensket_tilstand`-semantikk skal være eksplisitt og typespesifikk per entitetstype:
- Dokument: alltid `Ok` (satt av system ved registrering)
- Journalpost: `Journalfoert` eller `Avskrevet` avhengig av type (satt av system, obligatorisk livssyklus)
- Sak: klientstyrt og inkrementell. `OpprettSak` → ønsket `Opprettet`. `AvsluttSak` oppdaterer til `Avsluttet`.

R5 [5]: Rene domenefunksjoner driver overganger. Executoren spør domenet "gitt nåværende tilstander, hva er neste gyldige overgang?" Ingen IO i domenet.

R6 [5]: `command_execution` beholdes som scheduling/retry-autoritet. Entitetstabeller er verdensbilde. Retry-tellere, backoff, og "hent neste kommando" forblir på kommandonivå. Entitetsmodellen erstatter snapshot-tabeller og ephemeral plan, ikke kommandokøen.

Kode og schema bruker ASCII-stavemåten `oensket_tilstand`; prose kan omtale det som ønsket tilstand når feltet ikke siteres direkte.

R7 [5]: `tilstand_historikk`-tabell skal føre audit trail av alle tilstandsoverganger med `command_id` for mange-til-mange-sporing.

R8 [5]: `opprettet_av_command_id` skal være immutable provenance-felt på alle entitetsrader for å spore hvilken kommando opprettet raden.

R9 [5]: Feilet dokument (`FeiletPermanent`) gir irrecoverable terminal feil for kommandoen — ikke `Blocked`. Et permanent-feilet dokument kan aldri hentes, så kommandoen skal feile umiddelbart fremfor å vente i `blokkert_venter` for alltid.

R10 [5]: Rename `venter` → `blokkert_venter` i command_execution status for klarhet.

R11 [5]: Flere kommandoer kan dele samme sak-entitet. Kryss-kommando-avhengigheter håndteres via naturlige parent-oppslag.

### Forkastede alternativer

- **Step Table (Option A):** Forhåndsberegnede stegrader. Klar queryability men stale plans, mindre naturlig partial success.
- **Event Journal/Saga (Option B):** Append-only logg. Bra audit men vanskeligere å spørre live state.
- **Generisk `entity_tilstand`-tabell:** Bytter Postgres type safety for skjema-estetikk — dårlig trade.
- **Entity-drevet scheduling (uten command_execution):** Retry-logikk har ingen naturlig plass. Kompleks polling-query. Kommando-nivå statusrapportering brytes.

## Consequences

### Positive

- Full visibilitet i entitetsstate gjennom direkte database-queries
- Naturlig partial success når enkelte steg mislykkes mens andre fullføres
- Audit trail gjennom `tilstand_historikk` for alle overganger
- Typesikkerhet gjennom per-type tabeller med Postgres CHECK constraints
- Enklere debugging med persistent state fremfor ephemeral plan-deriving

### Negative

- `EksekveringsPlan`, `ResolvedPlan`, `ResolvedStep`, `plan_resolver`, `step_outcome` må fjernes
- `Ventegrunn` fjernes — venting er implisitt (prerequisites ikke møtt → domenet returnerer `None`)
- `EksekveringSnapshotRepository` og snapshot-tabeller (`sak_state`, `journalpost_state`, `dokument_state`) må fjernes
- `regler.rs` må utvides til fullstendige tilstandsmaskin-overgangsfunksjoner
- Integrasjonstester må skrives om til pure black-box (ingen direkte DB-tilgang)

### Tradeoffs

- Beholder `command_execution` for scheduling/retry selv om entitetsmodellen kunne tatt over — retry-logikk har ingen naturlig plass i entitetstabeller
- Per-type tabeller gir bedre typesikkerhet men mer skjema-kompleksitet enn én generisk tabell
- `tilstand_historikk` gir audit trail men introduserer mange-til-mange-relasjon via `command_id`

## Retirement

SKU-0002 er superseded by SKU-0007. R1, R2, R6, R7, R8, R10 og R11 er videreført i ånd, men modellen med `oensket_tilstand` som execution-driver i R3/R4 er forkastet. Ny modell er command executor-basert: entity state lagrer facts, og `CommandStateDecision` beregnes fra command execution state, entity facts og domeneregler.
