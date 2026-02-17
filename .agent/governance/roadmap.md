# Skuffen Veikart (Roadmap)

Dette dokumentet beskriver det overordnede veikartet for Skuffen-prosjektet.

## Mål
- Bygge et robust, idempotent arkivsystem (Skuffen).
- Sikre samsvar med regler for "Arkivfag".
- Sikker datahåndtering og strengt definerte feiltilstander (error states).

## Milepæler (Milestones)

### Fase 1: Core Command Ingestion & Idempotency
- **Fokus**: Motta commands sikkert og sikre at det ikke forekommer duplikater.
- **Nøkkelleveranser**:
    - NATS Command Listener.
    - Idempotens-lag (Dedup).
    - Grunnleggende validering (Schema).

### Fase 2: Domenelogikk & Arkivstatus-validering
- **Fokus**: Validere operasjoner mot domeneregler og **Arkivstatus (Archive State)**.
- **Mekanismer**:
    - **Domenevalidering**: Sikre at commands følger strenge regler for Arkivfag.
    - **Statusverifisering**: Hvis en command refererer til en eksisterende entitet (f.eks. "Notat på Sak B"), verifiser at den eksisterer i det eksterne Arkivet/Cache.
    - **Køhåndtering**: Sikre avhengighetsrekkefølge (f.eks. "Opprett Sak B" må skje før "Notat på Sak B").

### Fase 3: Operasjonsutførelse & Statushåndtering
- **Fokus**: Validert execution pipeline og definisjoner av feil.
- **Nøkkelleveranser**:
    - **Operation Executor**: System for å pushe validerte operasjoner til arkivet.
    - **Status-system**: NATS/DB-basert statushåndtering (TBD).
    - **Feedback Loop**: Publisering av events/status til klienter for spesifikke Command IDs.
    - **Feildefinisjoner**: Streng kategorisering av **Recoverable** vs **Irrecoverable** errors (definert i Sikri API).

### Fase 4: Admin CLI & Operasjonell kontroll
- **Fokus**: "CLI-lignende" Request-Reply grensesnitt for admins/klienter.
- **Funksjonalitet**:
    - **Introspeksjon**: `skuffen.<sak_ref>.status`-spørring for å se alle commands/ops og deres status.
    - **Recovery**: Mulighet til å manuelt "retry" feilede operasjoner etter å ha fikset ekstern tilstand.
