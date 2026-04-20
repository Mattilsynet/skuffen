# Skuffen - Partial success, best effort

Kilde: `.agent/guides/architecture/flows/Skuffen - Partial success, best effort.svg`

## Formål
- Diagrammet viser en flyt der behandling på samme sak kan fortsette selv om ett delsteg feiler.
- Hovedpoenget er å illustrere **partial success** i en **best effort**-modell: én feil stopper ikke automatisk all videre behandling.
- Diagrammet er en tekstlig hjelp til å lese SVG-en. Det er ikke en egen spesifikasjon som innfører nye regler utover det som vises i diagrammet og beskrives i repoets arkitekturguider.

## Kort oppsummert
- `sak A` opprettes først.
- Deretter går flyten videre i to grener på samme sak:
  - en gren for `journalpost X`
  - en gren for `journalpost Y`
- På `journalpost X` vises tre vedleggssteg (`U`, `V`, `K`). Ett av dem er markert som feil.
- Til tross for dette fortsetter flyten videre til `Journalfør journalpost X` og `Avskriv journalpost X`.
- Flyten for `journalpost Y` går videre uten markert feil.
- Når begge grener er kommet til sine viste sluttpunkter, samles de i `Avslutt sak A`.

## Hvordan lese diagrammet
- Flyten leses ovenfra og ned.
- Piler viser rekkefølge og avhengighet mellom steg.
- Når én boks forgrener seg til flere bokser, betyr det at flere etterfølgende steg springer ut fra samme tidligere tilstand i flyten.
- Når flere piler samles i én boks, betyr det at diagrammet viser en sammenføring før neste steg.
- Diagrammet har bokser med ulike farger, men SVG-en inneholder ingen egen legend som forklarer fargene eksplisitt. Derfor bør fargene leses forsiktig:
  - én boks er tydelig markert i rødt som feil
  - flere øvrige bokser er grønne eller grå/blålige
  - dokumentasjonen bør ikke tillegge disse fargene mer presis semantikk enn det diagrammet faktisk viser

## Stegvis gjennomgang av flyten

### 1. Opprett sak A
- Flyten starter med `Opprett sak A`.
- Dette er roten for resten av sekvensen.
- Alle senere operasjoner i diagrammet skjer i konteksten av den samme saken.

### 2. To videre grener på samme sak
Etter at saken er opprettet, deler flyten seg i to:

#### Gren 1: journalpost X
- `Opprett journalpost X på sak A`

#### Gren 2: journalpost Y
- `Opprett journalpost Y på sak A`

Diagrammet viser dermed at samme sak kan få flere journalposter som behandles som separate delsekvenser innenfor samme overordnede sakskontekst.

## Venstre gren: journalpost X

### 3. Opprett journalpost X
- Etter at `sak A` er opprettet, opprettes `journalpost X` på saken.
- Fra denne boksen går flyten videre til tre vedleggsoperasjoner.

### 4. Legg til vedlegg på journalpost X
Diagrammet viser tre vedleggskommandoer tilknyttet `journalpost X`:
- `Legg til vedlegg U på journalpost X`
- `Legg til vedlegg V på journalpost X`
- `Legg til vedlegg K på journalpost X`

Disse tre boksene er tegnet som parallelle etterfølgere av opprettelsen av journalposten. Diagrammet sier ikke eksplisitt om dette betyr faktisk parallell prosessering, eller bare at dette er tre separate steg som alle hører til samme del av flyten. Den tryggeste lesningen er derfor at alle tre er påfølgende eller relaterte vedleggssteg på `journalpost X`.

### 5. Markert feil på ett vedlegg
- I dagens SVG er boksen `Legg til vedlegg V på journalpost X` markert i rødt.
- Dette er den tydeligste visuelle indikasjonen på at dette steget representerer en feil eller en mislykket operasjon.
- De to andre vedleggsboksene på `journalpost X` (`U` og `K`) er ikke markert som feil i SVG-en.

### 6. Videre flyt etter den markerte feilen
- Pilene fra vedleggsstegene samles videre mot `Journalfør journalpost X`.
- Deretter går flyten til `Avskriv journalpost X`.
- Diagrammet viser altså at flyten for `journalpost X` fortsetter selv om ett vedleggssteg er markert som feil.

Dette er selve partial-success-signalet i diagrammet: resultatet for `journalpost X` er ikke "alt lykkes" eller "alt stopper", men at noe kan feile samtidig som senere steg likevel vises som gjennomført eller forsøkt.

## Høyre gren: journalpost Y

### 7. Opprett journalpost Y
- Den andre grenen fra `Opprett sak A` går til `Opprett journalpost Y på sak A`.

### 8. Videre behandling av journalpost Y
- Etter opprettelsen vises `Legg til vedlegg M på journalpost Y`.
- Deretter følger `Journalfør journalpost Y`.
- Så følger `Avskriv journalpost Y`.

I denne grenen er det ingen boks som er markert som feil i diagrammet.

## Sammenføring og avslutning av sak

### 9. Avslutt sak A
- Nederst i diagrammet møtes flyten fra `Avskriv journalpost X` og `Avskriv journalpost Y` i `Avslutt sak A`.
- Diagrammet viser dermed at saken avsluttes etter at de viste delsekvensene for begge journalposter har kommet til sine respektive sluttpunkter.

## Hva "best effort" betyr i denne sammenhengen

### Hva diagrammet selv viser
- Diagrammet viser at én feil i en del av flyten ikke nødvendigvis stopper resten av flyten for samme sak.
- Den konkrete illustrasjonen er at `Legg til vedlegg V på journalpost X` er markert som feil, mens senere steg for både `journalpost X` og `journalpost Y` fortsatt finnes i flyten.

### Hva arkitektur-guidene sier
I `.agent/guides/architecture/design_guidelines.md` beskrives best effort slik:
- Skuffen bruker en intern kommandokø for å kunne akseptere requests asynkront.
- Løsningen er laget for å kunne fortsette å fungere selv når underliggende arkivsystem er utilgjengelig.
- Best effort betyr at hvis en kommando feiler i konteksten av en sak, skal **neste lovlige kommando** utføres.
- En slik feil skal altså ikke blokkere hele køen.

Når dette leses sammen med diagrammet, er den naturlige tolkningen at den markerte feilen på ett vedlegg ikke stopper senere lovlige steg i samme saksforløp.

## Partial success-semantikk som diagrammet illustrerer
- Diagrammet illustrerer **delvis suksess**, ikke full suksess.
- Det betyr at resultatet for den samlede saken kan inneholde både:
  - steg som lykkes
  - steg som feiler
  - senere steg som fortsatt blir forsøkt eller gjennomført der det er lovlig
- Diagrammet viser derfor at vurderingen ikke skjer på nivået "hele saken lykkes eller hele saken feiler", men på nivået av enkeltkommandoer innenfor en sekvens.

## Viktig avgrensning: hva diagrammet ikke sier eksplisitt
Diagrammet er nyttig, men det viser ikke alt:

### Feilklassifisering er ikke eksplisitt vist
- Repoet bruker feilklassene `blocked`, `recoverable` og `irrecoverable`, jf. `.agent/guides/observability.md`.
- Diagrammet klassifiserer ikke den røde boksen eksplisitt i én av disse kategoriene.
- Det bør derfor ikke dokumenteres som sikkert at feilen er `blocked`, `recoverable` eller `irrecoverable` kun basert på fargen i SVG-en.

### "Neste lovlige kommando" er ikke detaljert operasjonalisert i figuren
- Designguiden sier at neste lovlige kommando skal kjøres.
- Selve diagrammet spesifiserer ikke i detalj hvordan denne lovlighetsvurderingen gjøres internt.
- Diagrammet viser bare utfallet: én feil stopper ikke hele flyten.

### Diagrammet viser ikke hele interne pipeline
- Designguiden beskriver at kommandoer går gjennom ingestion, validering, publisering på NATS, eksekvering og lagret execution state.
- Dette diagrammet er ikke en full pipeline-tegning av validator, executor, database og status-streams.
- Diagrammet bør derfor leses som en konseptuell flyt for kommandoresultater på sak/journalpost-nivå, ikke som en detaljert teknisk sekvens for alle interne komponenter.

### Diagrammet sier ikke eksplisitt om stegene er parallelle eller bare sideordnet tegnet
- De tre vedleggsstegene under `journalpost X` er tegnet side om side.
- SVG-en alene er ikke nok til å slå fast om dette betyr ekte parallell kjøring eller bare flere relaterte steg i samme del av flyten.

## Dokumentavvik i tidligere markdown
- Den tidligere teksten for dette diagrammet nevnte at `K` kunne være vedlegget som feilet.
- Dagens SVG viser derimot at det er `V` som er markert som feil.
- Den oppdaterte teksttolkningen bør derfor forholde seg til SVG-en som kilde og beskrive dette som et avvik i den gamle markdown-filen, ikke som en alternativ sannhet.

## Praktisk lesning av diagrammet
En praktisk og forsiktig lesning av figuren er:
- `sak A` opprettes.
- `journalpost X` og `journalpost Y` behandles som to delsekvenser innenfor samme sak.
- Ett vedlegg på `journalpost X` feiler.
- Flyten fortsetter likevel med videre steg som diagrammet fremstiller som lovlige å utføre.
- Hele saken trenger derfor ikke stoppe ved første feil.
- Dette er i tråd med repoets best-effort-prinsipp: feil på én kommando skal ikke automatisk blokkere hele køen eller hele sakens videre prosessering.

## Relaterte dokumenter
- `.agent/guides/architecture/design_guidelines.md`
- `.agent/guides/observability.md`
- `.agent/assets/diagram_text/Skuffen - General flow.md`
- `.agent/assets/diagram_text/Skuffen - Arkiv irrecoverable error.md`
