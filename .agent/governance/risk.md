# Protokoll for Risikoanalyse

Dette dokumentet definerer reglene for å analysere prosjektets veikart og plan for å identifisere potensielle risikoer.

## Sjekkliste for Analyse

Når man går gjennom `roadmap.md` og `plan.md`, eller foreslår nye arkitekturendringer, MÅ følgende spørsmål besvares:

### 1. Sikkerhet & Autentisering
- [ ] **Er auth-situasjonen håndtert skikkelig?**
    - Opprettholdes strenge grenser for autentisering og autorisering?
    - Håndteres hemmeligheter (secrets) sikkert (f.eks. ved bruk av `secrecy`-cratet, ikke logget)?
    - **Fase 4 Risiko**: Eksponerer Admin CLI sensitive operasjoner til uautoriserte brukere?
    - **Fase 4 Risiko**: Kan en bruker gjøre "retry" på en operasjon de bestemt ikke burde ha tilgang til?

### 2. Dataintegritet
- [ ] **Er det potensiale for tap av viktig data noen steder?**
    - Bevarer skjemaendringer eksisterende data?
    - **Fase 2 Risiko (State)**: Hva skjer hvis den lokale sjekken av "Arkivstatus" er utdatert (stale)? (Race condition: Sak B slettet eksternt, men vi tror den finnes).
    - **Fase 3 Risiko (Errors)**: Er skillet mellom Recoverable og Irrecoverable errors matematisk holdbart? (Risiko for uendelige retry-løkker eller tapt data).

### 3. Arkitektur & Vedlikeholdbarhet (Maintainability)
- [ ] **Er det tatt arkitekturbeslutninger som vil skape problemer senere?**
    - **Fase 3 Risiko**: Hvordan håndterer vi Status-systemet uten å bygge en distribuert monolitt?
    - **Fase 2 Risiko**: "Sjekk i arkivet" impliserer ekstern avhengighet og latency. Hvordan håndterer vi timeouts under validering?

### 4. Leverandørkjede & Avhengigheter
- [ ] **Minimerer dette angrepsflaten (attack surface)?**
    - Unngår vi unødvendige tunge avhengigheter?

## Risikologg

### 2026-01-22 - Oppdatering av Veikart
- **Identifisert**: Fase 2 validering krever kunnskap om ekstern state.
    - **Mitigering**: Trenger en cache-strategi eller "Optimistic Concurrency"-modell. Vi kan ikke blokkere hver command på et eksternt HTTP-kall.
- **Identifisert**: Fase 4 "Retry"-funksjonalitet tillater mutering av state fra et vilkårlig tidspunkt.
    - **Mitigering**: Streng validering må kjøres *på nytt* ved retry.
