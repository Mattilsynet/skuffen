## Eksempel
 nats request sak.hent '{
  "key": {
     "type": "arkivId",
     "value": "2025/513910"
  },
  "inkluderJournalposter": true
}
'
# skuffen

`skuffen` er en arkiveringstjeneste som ligger mellom interne systemer i Mattilsynet og Sikri sitt arkivsystem.

Tjenesten abstraherer bort et komplekst og upålitelig eksternt arkiv-API, og tilbyr et stabilt, asynkront og meldingsbasert grensesnitt for arkivering av saker og journalposter.

---

## Overordnet ansvar

Skuffen:
- mottar **kommandoer** fra interne tjenester
- oversetter disse til én eller flere **arkivoperasjoner**
- utfører operasjonene mot Sikri sitt arkiv når mulig
- lagrer og eksponerer status og resultater
- returnerer kun feil ved irrecoverable conditions

Tjenesten er *best effort* av design.

---

## Arkitektur

### Lagdeling

Tjenesten følger en strikt hexagonal arkitektur med CQRS-skille:
-Infrastructure
-Application
-Domain

- **Domain**
  - Ren forretningslogikk
  - Ingen eksterne avhengigheter
  - Regler for sak, journalpost og tilstandsovergang

- **Application**
  - Orkestrering
  - Porter (traits) for repositories, queries og eksterne adaptere

- **Infrastructure**
  - NATS
  - PostgreSQL
  - Sikri arkiv-API
  - Blob storage
  - Idempotency og køhåndtering

---

## Kommunikasjon

All integrasjon skjer over **NATS**.

- Skuffen kjører i én NATS account
- Andre tjenester kan ligge i andre accounts
- Ingen JWT over NATS
- Klienter publiserer meldinger og lytter på svar

---

## Data- og meldingsmodell

### Sekvens

En **sekvens** er en liste av kommandoer som hører logisk sammen.

```json
[
  {
    "kommando": "OpprettSak",
    "kommandoId": "uuid-1",
    "kommandoData": { }
  },
  {
    "kommando": "OpprettInngåendeJournalpost",
    "kommandoId": "uuid-2",
    "kommandoData": { }
  }
]
```

### Kommando

En kommando er en instruksjon fra et klientsystem.
	•	Asynkron
	•	Idempotent
	•	Mapper til én eller flere operasjoner

Eksempler:
	•	OpprettSak
	•	OpprettInngåendeJournalpost
	•	OpprettUtgåendeJournalpost
	•	AvsluttSak

Kommandoer er del av den offentlige kontrakten.



### Operasjon

En operasjon er en konkret handling mot arkivet.
	•	1:1 mapping mot et kall i Sikri sitt API
	•	Utføres sekvensielt
	•	Har egen status

Eksempler:
	•	OpprettJournalpost
	•	LeggTilVedlegg
	•	Journalfør
	•	Avskriv
	•	SendUt

Operasjoner er interne.


### Query

En query er et rent lesekall.
	•	Synkron
	•	Leser kun flate DTO-er
	•	Laster aldri domene-entiteter

Eksempler:
	•	Hent sak
	•	Hent journalpost
	•	Hent status for kommando eller sekvens

### Mapping

Skuffen har en stabil intern ID(skuffen-id) for alle entiteter. Denne er skjult for omverden.
Klienter sender med sin egen client-reference for entiteter som kan brukes til å referere til entieter. Eks. hvis arkivet et nede så kan man sende inn opprett sak<client-reference: 123> og senere sende inn OpprettJournalpost<client-reference: abc, sak: 123>.
Skuffen aksepterer også arkivet sine IDer.

Det finnes altså en mapping mellom alle 3:

client-reference <-> skuffen-id <-> arkiv-id

### Tilstandsmodell

#### Sak
	•	Under behandling
	•	Avsluttet


Avslutting kan ikke skje før alle journalposter er ferdig behandlet; avsrevet og journalført

#### Journalpost (forenklet)


Inngående / interne:
Opprettet → Journalført → Avskrevet

Utgående med utsending:
Opprettet → Ferdigstilt → Sendt → Journalført → Avskrevet

Utgående uten utsending:
Opprettet → Ferdigstilt → Journalført → Avskrevet




## Feilhåndtering

Skuffen følger best effort.
	•	Midlertidige feil lagres og retries
	•	Arkiv-nedetid skal ikke påvirke klienter
	•	Kommandoer avvises kun ved:
	•	valideringsfeil
	•	brudd på domeneinvarianter
	•	irrecoverable tekniske feil

Feil klassifiseres som:
	•	Recoverable (Prøves på nytt)
	•	Irrecoverable (Gir feilmelding tilbake til client, og stopper retries)



## Idempotens og sporbarhet
	•	Alle kommandoer har stabil kommandoId
	•	Idempotency-log hindrer dobbel utførelse
	•	Status kan alltid hentes i etterkant


## Kontraktsdeling

DTO typene ligger her:
https://github.com/Mattilsynet/landdyrtilsyn-libs/tree/master/lib-schemas/src/Skuffen

Disse utgjør den stabile kontrakten.

Infrastruktur oversetter mellom offentlig kontrakt og intern domene-modell.

Målgruppe

Primært:
	•	Utviklere i Mattilsynet som integrerer mot arkiv

Sekundært:
	•	Eksterne som vil forstå arkitekturen

Repoet er public, men tjenesten er bygget for intern bruk.
