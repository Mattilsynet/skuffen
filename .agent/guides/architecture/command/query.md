# Queries

| Kommando | Beskrivelse | Notater |
| :--- | :--- | :--- |
| Hent sak | Henter en sak, med eller uten journalposter. | Subject: `arkiv.request.sak.hent`. Implemented by `HentSakService` implementing `HentSakUseCase`. Henter kun metadata intil videre. Dokumenter og vedlegg kommer senere ved endring i sikri api |
| Hent Journalpost | Henter en journalpost. | Subject: `arkiv.request.journalpost.hent`. Backing repository er foreløpig fake/testdata. Henter kun metadata intil videre. Dokumenter og vedlegg kommer senere ved endring i sikri api |
| Hent bruker/MT-enheter | Henter brukerens MT-enheter. | Subject: `arkiv.request.bruker.mt_enheter`. Live stub som returnerer `NatsResponse::Error { message: "Not implemented" }` inntil kontrakt og implementation er avklart. |
| Hent Dokument | Henter et dokument fra enten sikri eller internt blob storage. | Ikke i scope enda |
