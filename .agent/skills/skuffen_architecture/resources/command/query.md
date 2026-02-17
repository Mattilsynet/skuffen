# Queries

| Kommando | Beskrivelse | Notater |
| :--- | :--- | :--- |
| Hent sak | Henter en sak, med eller uten journalposter. | Implemented by `HentSakService` implementing `HentSakUseCase`. Orchestrates fetching via `SakPort`. Henter kun metadata intil videre. Dokumenter og vedlegg kommer senere ved endring i sikri api |
| Hent Journalpost | Henter en journalpost. | Planned: `HentJournalpostUseCase`. Henter kun metadata intil videre. Dokumenter og vedlegg kommer senere ved endring i sikri api |
| Hent Dokument | Henter et dokument fra enten sikri eller internt blob storage. | Ikke i scope enda |

