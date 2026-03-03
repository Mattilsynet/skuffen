# Eksekvering av kommandoer mot Sikri

Dette dokumentet beskriver hvordan Skuffen eksekverer kommandoer mot Sikri. Fokus er best‑effort, rekkefølge, arkivfag og konsekvent feilhåndtering.

## Flyt

1. Lytt på `arkiv.command.ready.<entity>.<commandId>` (JetStream, retention 180 dager).
2. Lagre kommandoen i `command_execution` (inkl. payload). ACK meldingen når DB‑innsett er ok.
3. Eksekvering worker henter klare kommandoer fra DB og kjører dem.
4. Worker bygger eksekveringsplan (parse + typebasert validering).
5. Worker kjører planen steg for steg:
   - Hvert steg har en guard som leser DB‑state og avgjør om vi kan kjøre, skal skippe, eller må blokkere.
   - Når guard sier “kjør”: kall Sikri, oppdater state og id‑mapping.
6. Emit `CommandStatusEvent` på `arkiv.status.<commandId>`.
7. Når kommandoen er terminal: publiser `arkiv.command.done.<entity>.<commandId>`.

## Arkivfaglige regler (oppsummering)

Hent detaljer fra arkivfag‑ressurser:
- Sak: `.agent/skills/arkivfag/resources/sak.md`
- Journalpost: `.agent/skills/arkivfag/resources/journalpost/*`
- Dokument: `.agent/skills/arkivfag/resources/dokument.md`

Kjernekrav:
- Sak må finnes før journalpost kan opprettes.
- Saksstatus `B` eller `F` tillater journalposter. `A` (avsluttet) låser saken.
- Avslutt sak krever at alle journalposter er ferdige:
  - Inngående: journalført og avskrevet.
  - Utgående: journalført.
  - Internt notat: journalført.

 Utgående uten utsending:
- Opprett i `R`, sett til `J`.

## Eksekveringsplan (parse + typebasert validering)

Kommandoer blir oversatt til en plan av steg. Typebasert validering brukes for å sikre at steg har nødvendige felt før de kan kjøres (f.eks. at utgående har mottaker og hoveddokument).

### Opprett sak
Steg:
- `OpprettSak`

### Opprett journalpost (inngående/utgående/internt)
Steg:
- `OpprettJournalpost`
- `LeggTilDokument` (ett steg per dokument, ingen øvre grense)
- `Journalfør` (avhenger av type og flyt)
- `Avskriv` (kun inngående)

### Avslutt sak
Steg:
- `AvsluttSak`

## Lokal state i database (arkivfaglige fakta)

Minimal lagring for rekkefølge og blokkering.

### SakState
- `sak_id` (skuffen_id)
- `saksnummer` (nullable)
- `status` (B/F/A)
- `opprettet` bool

### JournalpostState
- `journalpost_id` (skuffen_id)
- `sak_id` (FK)
- `journalpostnummer` (nullable)
- `type` (I/U/X)
- `med_utsending` bool (kun U)
- `journalført` bool
- `avskrevet` bool (kun I)
- `ekspedert` bool (kun U, kun ved utsending)
- `har_feilede_dokumenter` bool

### DokumentState
- `dokument_id` (skuffen_id)
- `journalpost_id` (FK)
- `lagt_til` bool
- `irrecoverable_feil` bool

### CommandExecution
- `command_id`
- `correlation_id`
- `payload` (CommandEnvelope som JSON)
- `status` (pending|running|ok|blocked|error|retrying)
- `attempts`, `last_error`, `next_retry_at`
- `locked_at`, `locked_by`

## Rekkefølge og blokkering

### Opprett journalpost
Krav:
- Sak finnes og er ikke avsluttet.
- Hvis `SakKey::ClientReference` mangler arkiv_id, kan eksekvering fortsette på skuffen‑state (best effort). Journalpost blir registrert lokalt og avventer arkiv_id.

### Legg til dokument
Krav:
- Journalpost må være opprettet.
- Ingen øvre grense på antall vedlegg.

### Journalfør
Krav:
- Journalpost er opprettet.
- Alle dokumenter som skal inngå er lagt til.

### Avskriv
Krav:
- Kun inngående.
- Journalpost må være journalført.

### Avslutt sak
Krav:
- Alle journalposter på saken er ferdige iht. arkivfag (se over).
- Ingen journalposter har `har_feilede_dokumenter = true`.

 Hvis krav ikke er oppfylt: `Blocked`.

## Hva hvis prosessen dør midt i planen?

- Planen ligger i RAM, men **hver operasjon er idempotent** og **gated av DB‑state**.
- På restart blir hele kommandoen re‑prosessert fra start, men steg som allerede er fullført blir skippet.
- Eksempel: `OpprettJournalpost` fullført, `LeggTilDokument` feilet → på retry vil `OpprettJournalpost` skippe fordi `journalpost_state` finnes.

## Timeout/feil fra arkivet

- Sikri‑feil mappes til `Recoverable` eller `Irrecoverable`.
- Recoverable feil (timeout, 5xx, 429) fører til `CommandStatus::Retrying` og planlagt `next_retry_at` i DB.
- NATS redelivery brukes kun hvis DB‑innsett feiler (ikke for retry).

## Best effort og delvis suksess

Hvis ett vedlegg feiler irrecoverably, stopper ikke systemet hele sekvensen. Det påvirker kun journalposten det gjelder, og blokkerer avslutning av sak.

Eksempel:
1. Opprett sak A
2. Opprett journalpost X med vedlegg 1,2,3
3. Opprett journalpost Y med vedlegg 4
4. Avslutt sak A

Hvis vedlegg 2 feiler irrecoverably:
- Vedlegg 3 legges fortsatt til.
- Journalpost Y opprettes og fullføres.
- `AvsluttSak` blokkeres fordi journalpost X ikke er komplett.

## Feilhåndtering

Samme modell som validering, men styrt fra DB‑worker:

- **Recoverable**: 429/5xx/timeout → `CommandStatus::Retrying`, `next_retry_at` settes.
- **Irrecoverable**: 4xx som ikke kan rettes → `CommandStatus::Error` (terminal).
- **Blocked**: domenekrav ikke oppfylt → `CommandStatus::Blocked` + `next_retry_at`.

`CommandStatusEvent` sendes på alle overganger (Pending/Retrying/Ok/Error/Blocked).

## Backoff‑strategi

Backoff skal aldri være 0. Bruk eksponentiell backoff med øvre grense til ett forsøk per dag, og fortsett med ett forsøk per dag deretter.

Eksempel:
- 1m, 5m, 15m, 1h, 6h, 12h, 24h, 24h, 24h ...

## NATS‑kanaler

- Input: `arkiv.command.ready.<entity>.<commandId>`
- Status: `arkiv.status.<commandId>` (stream `arkiv_status`)
- Done: `arkiv.command.done.<entity>.<commandId>`

`<entity>` er `sak` eller `journalpost`.
