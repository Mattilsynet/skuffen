# Commands

| Command | Operasjoner | Beskrivelse | Notater |
| :--- | :--- | :--- | :--- |
| Opprett sak | Opprett Sak. | Oppretter en sak. Får status "under behandling" | Planned: `src/domain/src/model/operasjon/opprett_sak.rs`, `SakPort::opprett` (Infra: `sikri_client::create_sak`) |
| Avslutt sak | Avslutt sak | Avslutter en sak. | Kan IKKE journalfore og avsrive alle journalposter; dan kan uferdige ting ligge igjen, og det kan være at dokumenter har kommet inn til saken fra andre steder og har ikke blitt sett/behandlet av et menneske. Planned: `SakPort::avslutt` |
| Opprett Journalpost Internt notat | Opprett Journalpost av type X, Journalfør, avskriv | | Planned: `src/domain/src/model/operasjon/journalfør.rs`, `avskriv.rs` |
| Opprett Journalpost internt notat med vedlegg | Opprett Journalpost av type X, [for v in vedlegg: legg til vedlegg], Journalfør, avskriv | | Planned: `legg_til_vedlegg.rs` |
| Opprett Journalpost Inngående | Opprett Journalpost av type I, Journalfør, avskriv | | |
| Opprett Journalpost Inngående med vedlegg | Opprett Journalpost av type I, [for v in vedlegg: legg til vedlegg], Journalfør, avskriv | | |
| Opprett Journalpost Utgående | Opprett Journalpost av type U, Journalfør, avskriv | | |
| Opprett Journalpost Utgående med vedlegg | Opprett Journalpost av type U, [for v in vedlegg: legg til vedlegg], Journalfør, avskriv | | |
| Opprett Journalpost Utgående med vedlegg og utsending | Opprett Journalpost av type U, [for v in vedlegg: legg til vedlegg], Send ut, avskriv | | |
| Opprett Journalpost Utgående med utsending | Opprett Journalpost av type U, send ut, avskriv | | |


