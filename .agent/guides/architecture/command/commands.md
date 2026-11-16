# Commands

| Command | Beskrivelse | Notater |
| :--- | :--- | :--- |
| Opprett sak | Oppretter en sak og starter saksbehandling. | Saksbehandler, enhet eller begge må være satt. Sikri fyller ut enhet når bare saksbehandler er satt. |
| Avslutt sak | Avslutter en sak. | Kan IKKE journalføre og avskrive alle journalposter; da kan uferdige ting ligge igjen, og det kan være at dokumenter har kommet inn til saken fra andre steder og har ikke blitt sett/behandlet av et menneske. Planned: `SakPort::avslutt` |
| Opprett Journalpost Internt notat | Oppretter journalpost av type internt notat, journalfører og avskriver. | |
| Opprett Journalpost internt notat med vedlegg | Oppretter journalpost av type internt notat med vedlegg, journalfører og avskriver. | |
| Opprett Journalpost Inngående | Oppretter journalpost av type inngående, journalfører og avskriver. | |
| Opprett Journalpost Inngående med vedlegg | Oppretter journalpost av type inngående med vedlegg, journalfører og avskriver. | |
| Opprett Journalpost Utgående | Oppretter journalpost av type utgående, journalfører og avskriver. | |
| Opprett Journalpost Utgående med vedlegg | Oppretter journalpost av type utgående med vedlegg, journalfører og avskriver. | |
| Opprett Journalpost Utgående med vedlegg og utsending | Oppretter journalpost av type utgående med vedlegg, sender ut og avskriver. | |
| Opprett Journalpost Utgående med utsending | Oppretter journalpost av type utgående, sender ut og avskriver. | |

## Tildeling ved opprettelse av sak

`OpprettSak.saksbehandler_id` og `saksbehandler_enhet` er `Option<String>`.
Utelatt felt eller `null` betyr usatt; serialisering utelater usatte felt.
Minst ett felt må være satt, og oppgitte verdier må ikke være tomme eller bare
whitespace. Libs beskriver feltene uten tildelingsvalidering. Skuffen sjekker
tildelingen i batchkontrollen ved mottak, før noe lagres eller sendes videre.
Ugyldig tildeling avviser hele batchen med en kvittering som angir kommando,
årsak og hvordan feltene skal fylles ut, sammen med eventuelle andre batchfeil.

Fravær bevares som `NULL` i PostgreSQL og som utelatt felt i Sikri-requesten.
Skuffen utleder ikke enhet selv. En sak til fordeling kan også hentes uten
saksbehandler; query-responsen har allerede valgfrie saksbehandlerfelt.
Journalposter og `SettSaksansvarlig` beholder sine eksisterende kontrakter.

### Kompatibilitet og utrulling

Eksisterende klienter som sender begge felt som strenger støttes uendret på
samme subject og command-navn. Det kreves ingen samtidig klientoppgradering.
Eier har bekreftet at eksisterende klienter ikke sender blanke verdier.

1. Publiser schemas-endringen som en ny tag i `landdyrtilsyn-libs`;
    eksisterende tags endres ikke.
2. Skuffen bruker `lib-schemas` og `lib-sql` fra publisert tag `1.8.1`, med
   `Cargo.lock` som resolved boundary. `lib-nats` beholdes på `1.7.7`.
   `lib-sql` bruker Postgres-backend direkte; Skuffen aktiverer derfor `macros`
   på sin egen SQLx-avhengighet for `sqlx::migrate!`.
3. Rull ut Skuffen og vent til gamle instanser er ute, før klienter begynner å
   sende meldinger med bare ett felt. Dette gjelder hele command-pipelinen,
   ikke bare request-reply-mottaket.
4. Klienter kan oppgradere uavhengig etterpå. Rust-kode som tar inn den nye
   schemas-taggen må bruke `Some(...)`/`None` for de to feltene.

Gamle Skuffen-instanser kan ikke lese meldinger som utelater et felt.
Rollback til gammel Skuffen krever derfor at nye meldingsformer stoppes og
at slike meldinger i inbox/ready-strømmene og uferdige saker med `NULL` i ett
tildelingsfelt håndteres før rollback.
