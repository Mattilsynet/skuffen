# Commands

| Command | Beskrivelse | Notater |
| :--- | :--- | :--- |
| Opprett sak | Oppretter en sak og starter saksbehandling. | Planned: `SakPort::opprett` (Infra: `sikri_client::create_sak`) |
| Avslutt sak | Avslutter en sak. | Kan IKKE journalføre og avskrive alle journalposter; da kan uferdige ting ligge igjen, og det kan være at dokumenter har kommet inn til saken fra andre steder og har ikke blitt sett/behandlet av et menneske. Planned: `SakPort::avslutt` |
| Opprett Journalpost Internt notat | Oppretter journalpost av type internt notat, journalfører og avskriver. | |
| Opprett Journalpost internt notat med vedlegg | Oppretter journalpost av type internt notat med vedlegg, journalfører og avskriver. | |
| Opprett Journalpost Inngående | Oppretter journalpost av type inngående, journalfører og avskriver. | |
| Opprett Journalpost Inngående med vedlegg | Oppretter journalpost av type inngående med vedlegg, journalfører og avskriver. | |
| Opprett Journalpost Utgående | Oppretter journalpost av type utgående, journalfører og avskriver. | |
| Opprett Journalpost Utgående med vedlegg | Oppretter journalpost av type utgående med vedlegg, journalfører og avskriver. | |
| Opprett Journalpost Utgående med vedlegg og utsending | Oppretter journalpost av type utgående med vedlegg, sender ut og avskriver. | |
| Opprett Journalpost Utgående med utsending | Oppretter journalpost av type utgående, sender ut og avskriver. | |
