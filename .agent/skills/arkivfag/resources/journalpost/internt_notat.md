# 📝 Journalposttype X – Internt notat uten mottaker

**X-notat** brukes for **interne dokumenter uten mottaker**: f.eks. interne fagnotater, arbeidsunderlag, metodedokumentasjon, referater som ikke sendes, mv.

> Internt notat skal **ikke** brukes i kommunikasjon mellom hovedkontoret og tilsynet – der skal det brukes I/U (inngående/utgående). Dette følger av instruksen om samhandling mellom Mattilsynets to forvaltningsorganer.

## 🧩 Nøkkelpunkter

- **Journalposttype = "X"**
- `journalstatus` settes **ikke** ved opprettelse — Elements åpner journalposten
  i en status der endringer er mulige.
- Journalføring (`J`) er et **eget steg etterpå**, når alle dokumenter er på plass.
- **Ingen mottakere** (`avsendereMottakere` kan være tom liste).
- Minst **ett hoveddokument** må følge journalposten.
- Publiseres til eInnsyn når status = **J** (om ikke skjermet).

> ⚠️ En journalført journalpost er **låst**. Opprettes den direkte i `J`, kan
> vedlegg ikke legges til i ettertid.

## 🛠️ Obligatoriske felter

| Felt                 | Type     | Påkrevd?   | Kommentar                                   |
| :------------------- | :------- | :--------- | :------------------------------------------ |
| `tittel`             | string   | ✅         | Tittel på journalposten                     |
| `dokumentDato`       | datetime | ✅         | Dato dokumentet ble opprettet               |
| `journalposttype`    | string   | ✅         | Alltid `"X"`                                |
| `journalstatus`      | string   | ⛔         | Settes **ikke** ved opprettelse. Journalføres i eget steg |
| `avskrivDirekte`     | boolean  | 🛡️         | Ikke relevant for X-notater                 |
| `avskrivningsmaate`  | string   | 🛡️         | Ikke relevant for X-notater                 |
| `saksbehandler`      | string   | ✅         | ID for ansvarlig saksbehandler              |
| `saksbehandlerEnhet` | string   | ✅         | Enhet/avdeling som har produsert notatet    |
| `avsendereMottakere` | array    | 🛡️         | Skal ikke benyttes                          |
| `dokumenter`         | array    | ✅         | Minst ett dokument med `hoveddokument=true` |
| `tilgangskode`       | string   | 🛡️/valgfri | Brukes ved skjerming                        |
| `tilgangshjemmel`    | string   | 🛡️/valgfri | Skal alltid settes sammen med tilgangskode  |
