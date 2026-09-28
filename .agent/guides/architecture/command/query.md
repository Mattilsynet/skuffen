# Queries

| Kommando | Beskrivelse | Notater |
| :--- | :--- | :--- |
| Hent sak | Henter en sak uten journalposter. | Subject: `arkiv.request.sak.hent`. Implemented by `HentSakService` implementing `HentSakUseCase`. |
| Hent sak med journalposter | Henter en sak med journalposter og dokumentmetadata i ett arkivkall. | Subject: `arkiv.request.sak.med_journalposter`. Samme `HentSakService`, med inkludering alltid på. Hoveddokumentet er første dokument; hvert dokument har arkivets `dokument_id`. `dokument_dato` er sonefri `NaiveDateTime`. Dokumentinnhold (filer) kommer senere ved endring i sikri api. |
| Hent Journalpost | Henter en journalpost. | Subject: `arkiv.request.journalpost.hent`. Backing repository er foreløpig fake/testdata. Henter kun metadata intil videre. Dokumenter og vedlegg kommer senere ved endring i sikri api |
| Hent bruker/MT-enheter | Henter brukerens MT-enheter. | Subject: `arkiv.request.bruker.mt_enheter`. Live stub som returnerer `NatsResponse::Error { message: "Not implemented" }` inntil kontrakt og implementation er avklart. |
| Hent Dokument | Henter et dokument fra enten sikri eller internt blob storage. | Ikke i scope enda |

## Adressering av sak ved lesing

`arkivId` (saksnummer) går direkte til arkivet; Skuffen trenger ikke kjenne
saken. `clientReference` slås opp til saksnummer gjennom entitetsregisteret
(`SaksnummerOppslag`), og må tilhøre en sak. Query-flyten oppretter aldri
identitet eller state; command- og operasjonsflyten er uberørt.

## Offentlig query mot admin read

De to kanalene svarer på forskjellige spørsmål og skal ikke blandes.

| | Offentlig query (`arkiv.request.*`) | Admin read (`arkiv.admin.read.*`) |
| :--- | :--- | :--- |
| Formål | Klientvendt oppslag av arkivets tilstand | Lokal tilstand en reparasjon må forstås ut fra |
| Kilde | Live-oppslag mot arkivet/Sikri | Bare persistert PostgreSQL-state |
| Synlighet | `skuffen_id`, operasjoner og intern kontekst er skjult | Alt eksponeres bevisst; det trengs for å adressere riktig mål |
| Validering | Responser rapporterer tilstand uten å revalidere (SKU-0015 R7) | Samme prinsipp: lagrede koder og fritekst returneres som strings |
| Attribusjon | Ingen | Obligatorisk `utfort_av`, logget én gang per request |

Admin read leser aldri status-streamen, arkivet eller object store, og skriver
aldri. Se ADR `SKU-0018`.
