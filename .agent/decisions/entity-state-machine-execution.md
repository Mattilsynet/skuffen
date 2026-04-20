# Beslutning: Entity State Machine Execution Model

**Status:** Besluttet  
**Dato:** 2026-04-20

## Kontekst

Det nåværende eksekveringssystemet re-deriverer en in-memory plan hver gang en kommando kjøres.
Det finnes ingen per-steg persistering — man kan ikke se i databasen hvor en kommando er i sin eksekvering.
Systemet skal bygges om for debuggbarhet, sikkerhet, og partial-success-støtte.

## Beslutning

### Modell: Entity State Machines

Erstatt ephemeral plan + snapshot-tabeller med **persisterte tilstandsmaskiner per domeneentitet** (sak, journalpost, dokument).

### Nøkkelvalg

1. **Per-type tilstandstabeller** (`sak_tilstand`, `journalpost_tilstand`, `dokument_tilstand`) — ikke én generisk tabell. Entitetstypene har fundamentalt ulike tilstandsformer. Postgres CHECK constraints + Rust enums gir reell typesikkerhet.

2. **`tilstand` + `ønsket_tilstand`** på hver entitetsrad. `tilstand` er nåtilstand (fakta), `ønsket_tilstand` er ønsket slutt (intensjon). Executoren closer gapet.

3. **`ønsket_tilstand`-semantikk varierer per type:**
   - Dokument: alltid `Ok` (satt av system ved registrering)
   - Journalpost: `Journalfoert` eller `Avskrevet` avhengig av type (satt av system, obligatorisk livssyklus)
   - Sak: klientstyrt og inkrementell. `OpprettSak` → ønsket `Opprettet`. `AvsluttSak` oppdaterer til `Avsluttet`.

4. **Rene domenefunksjoner driver overganger.** Executoren spør domenet "gitt nåværende tilstander, hva er neste gyldige overgang?" Ingen IO i domenet.

5. **`command_execution` beholdes som scheduling/retry-autoritet.** Entitetstabeller er verdensbilde. Retry-tellere, backoff, og "hent neste kommando" forblir på kommandonivå. Entitetsmodellen erstatter snapshot-tabeller og ephemeral plan, ikke kommandokøen.

6. **`tilstand_historikk`-tabell** for audit trail av alle overganger.

7. **`opprettet_av_command_id`** som immutable provenance-felt på entitetsrader. `command_id` på historikkrader for mange-til-mange-sporing.

8. **Feilet dokument (`FeiletPermanent`) gir irrecoverable terminal feil for kommandoen — ikke `Blocked`.** Et permanent-feilet dokument kan aldri hentes, så kommandoen skal feile umiddelbart fremfor å vente i `blokkert_venter` for alltid.

9. **Rename: `venter` → `blokkert_venter`** i command_execution status for klarhet.

10. **Flere kommandoer deler samme sak-entitet.** Kryss-kommando-avhengigheter er naturlige parent-oppslag.

### Forkastede alternativer

- **Step Table (Option A):** Forhåndsberegnede stegrader. Klar queryability men stale plans, mindre naturlig partial success.
- **Event Journal/Saga (Option B):** Append-only logg. Bra audit men vanskeligere å spørre live state.
- **Generisk `entity_tilstand`-tabell:** Bytter Postgres type safety for skjema-estetikk — dårlig trade.
- **Entity-drevet scheduling (uten command_execution):** Retry-logikk har ingen naturlig plass. Kompleks polling-query. Kommando-nivå statusrapportering brytes.

## Konsekvenser

- `EksekveringsPlan`, `ResolvedPlan`, `ResolvedStep`, `plan_resolver`, `step_outcome` fjernes
- `Ventegrunn` fjernes — venting er implisitt (prerequisites ikke møtt → domenet returnerer `None`)
- `EksekveringSnapshotRepository` og snapshot-tabeller (`sak_state`, `journalpost_state`, `dokument_state`) fjernes
- `regler.rs` utvides til fullstendige tilstandsmaskin-overgangsfunksjoner
- Integrasjonstester er skrevet om til pure black-box (ingen direkte DB-tilgang)
