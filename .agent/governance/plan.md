# Skuffen Implementasjonsplan

Dette dokumentet detaljerer den spesifikke implementasjonsplanen for kommende funksjoner og endringer.

## Nåværende fokus: Overgang Fase 1 & 2
Vi ferdigstiller nå Command Ingestion og beveger oss mot Domenevalidering.

### Aktiv arbeidsstrøm
1.  **Command Ingestion (Fase 1)**
    - Ferdigstille NATS listener.
    - Verifisere Idempotency-sjekker.

2.  **Domenevalidering (Fase 2 Forb.)**
    - Implementere `Validator`-traits for Commands.
    - Designe mekanismen for oppslag av "Arkivstatus" (Mock vs Real).

## Backlog (Neste steg)
- [ ] Designe Operation Executor (Fase 3).
- [ ] Definere "Recoverable" vs "Irrecoverable" errors.
- [ ] Prototype Admin CLI-grensesnitt (Fase 4).
