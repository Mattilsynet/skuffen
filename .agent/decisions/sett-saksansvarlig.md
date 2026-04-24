# Beslutning: SettSaksansvarlig — Noark 5 M306 i tilstandsmaskinen

**Status:** Besluttet  
**Dato:** 2026-04-23

## Kontekst

Skuffen trengte en ny `SettSaksansvarlig`-kommando for å kalle Sikris `PUT /api/Archive/SetSaksansvarligIdForArkivSak`. Spørsmålet var om saksansvarlig er et arkivkonsept (skal modelleres i tilstandsmaskinen) eller et saksbehandlingskonsept (skal behandles som pass-through).

## Forskning: Noark 5 tjenestegrensesnitt v1.1

Fra standarden (nasjonalarkivet/noark5-tjenestegrensesnitt-standard, versjon 1.1, 2023-06-09):

**Saksmappe** (spesialisering av Mappe) har disse relevante attributtene:

| Attributt | Metadata-kode | Forekomst | Beskrivelse |
|---|---|---|---|
| `saksansvarlig` | M306 | [0..1] [1..1] | Navn på person som er saksansvarlig. Registreres automatisk eller overstyres manuelt. |
| `referanseSaksansvarlig` | — | [0..1] [1..1] | SystemID-referanse til saksansvarlig. |
| `administrativEnhet` | M305 | [0..1] [1..1] | Enhet med ansvar for saksbehandlingen. |

Nøkkelkrav fra standarden:

- **6.1.13:** Når en Saksmappe settes til avsluttet, skal det **ikke** være mulig å endre: `saksdato`, `administrativEnhet`, `saksansvarlig`. Dvs. saksansvarlig **låses** ved avslutting.
- **6.1.14:** Øvrige metadata på en avsluttet Saksmappe **bør** fortsatt kunne endres (med logging).

**Konklusjon:** Saksansvarlig er **førsteklasses Noark 5 arkiv-metadata** (M306) på Saksmappe. Det er en del av den formelle arkivposten som bevares i arkivuttrekk.

## Arkitekturprinsipp

Skuffen skal være nær arkivdomenet (Noark 5) og ikke saksbehandlingsdomenet. Noark 5 er den styrende standarden.

## Beslutning

### Saksansvarlig modelleres i tilstandsmaskinen

Saksansvarlig er kjernearkivmetadata og skal modelleres med `ønsket` vs `nåværende`-mønsteret — samme prinsipp som `tilstand`/`oensket_tilstand` for sak, journalpost og dokument.

### Nøkkelvalg

1. **`oensket_saksansvarlig` + `naavaerende_saksansvarlig`** som `Option<Saksansvarlig>` på `SakMedBarn`, der `Saksansvarlig` er en navngitt value object med `saksbehandler_id` og `enhet`. Begge felt er `None` når ingen saksansvarlig er forespurt.

2. **Step 1b** i `neste_handling()`: Rett etter OpprettSak, før journalpostarbeid. Krever `saksnummer.is_some()` for å unngå blocked-retry-syklus (saksnummer trengs for Sikri-kallet). Saksansvarlig settes tidlig fordi det er metadata på saken, ikke avhengig av journalposter.

3. **AvsluttSak-vakt (Noark 5 6.1.13):** En sak kan ikke avsluttes med mindre `oensket_saksansvarlig == naavaerende_saksansvarlig`. Hvis de ikke matcher, returnerer tilstandsmaskinen `SettSaksansvarlig` i stedet for `AvsluttSak`.

4. **`er_ferdig()`** sjekker også saksansvarlig-likhet.

5. **Idempotent:** Hvis `oensket == naavaerende` returnerer maskinen `None` → ingen operasjon → `er_ferdig` → success.

6. **Ny migrasjon** (`20260423120000_add_sett_saksansvarlig.up.sql`): Legger til 4 kolonner på `sak_tilstand` og utvider CHECK-constraints på `command_execution`.

### Forkastet alternativ: Pass-through

Et tidligere design foreslo å behandle `SettSaksansvarlig` som en pass-through som omgår tilstandsmaskinen. Dette ble forkastet fordi:
- Noark 5-forskning beviste at saksansvarlig er arkivmetadata, ikke bare saksbehandlingsmetadata.
- En sak kan ikke avsluttes uten at saksansvarlig er korrekt satt.
- Pass-through ville ikke fanges opp av `er_ferdig()`, og AvsluttSak ville ikke kunne vente på at saksansvarlig er satt.

## Observabilitet

- Sikri-kallet logges med `saksnr` men **ikke** `saksbehandler_id` eller `enhet` — disse kan inneholde personidentifiserbare opplysninger og skal ikke i logger (safe logging-prinsipp).

## Konsekvenser

- Tilstandsmaskinen er noe mer kompleks (nytt steg 1b, ny vakt på steg 8).
- Alle som konstruerer `SakMedBarn` må inkludere de nye feltene.
- Ny migrasjon kreves for eksisterende databaser (med tilhørende down-migrasjon for rollback).
- `Saksansvarlig` value object brukes i domain, infrastructure og tests — gir typesikkerhet og lesbarhet.
