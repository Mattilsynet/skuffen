# Utgående

# 📤 Journalposttype U – Utgående saksdokumenter

Utgående journalposter brukes når Mattilsynet sender **saksdokumenter** ut til eksterne parter (brev, vedtak, svar på henvendelser, informasjon). Dokumentene registreres i arkivet og ekspederes, før de journalføres og publiseres. Statusløpet varierer avhengig av om dokumentet sendes via SvarUt eller ikke.

## 🧩 Nøkkelpunkter

- **Journalposttype = "U"**
- Brukes for all kommunikasjon som går _fra_ Mattilsynet _til eksterne mottakere_.
- Må ha minst **én mottaker** (`erMottaker=true`).
- Må ha minst **ett hoveddokument** (`hoveddokument=true`).
- Opprettes normalt i **status R (Reservert)** som default verdi ved opprettelse
  - Dersom dokumentet skal sendes via **SvarUt**, endres status fra R til F.
    Dersom dokumentet **ikke skal sendes via SvarUt**, endres status fra R til E.
- `avskrivDirekte` settes **ikke** for utgående dokumenter.
- Publiseres automatisk til eInnsyn når status = J (hvis ikke skjermet).

## 🛠️ Obligatoriske felter ved opprettelse

| Felt                  | Type     | Påkrevd?   | Kommentar                                                                                                                                       |
| :-------------------- | :------- | :--------- | :---------------------------------------------------------------------------------------------------------------------------------------------- |
| `tittel`              | string   | ✅         | Tittel på journalpost                                                                                                                           |
| `dokumentDato`        | datetime | ✅         | Dato dokumentet er produsert/sendt                                                                                                              |
| `journalposttype`     | string   | ✅         | Alltid `"U"`                                                                                                                                    |
| `journalstatus`       | string   | ✅         | Settes til `"R"` ved opprettelse. <br><br>_Endres fra **R til F** ved bruk av SvarUt _<br>_Endres fra **R til E** dersom ikke svar ut benyttes_ |
| `avskrivDirekte`      | boolean  | 🛡️         | Skal ikke brukes for utgående dokumenter                                                                                                        |
| `avskrivningsmaate`   | string   | 🛡️         | Ikke relevant for utgående dokumenter                                                                                                           |
| `saksbehandler`       | string   | ✅         | ID for ansvarlig saksbehandler                                                                                                                  |
| `saksbehandlerEnhet`  | string   | ✅         | Enhet/avdeling som har sendt ut dokumentet                                                                                                      |
| `avsendereMottakere`  | array    | ✅         | Minst én mottaker (`erMottaker=true`)                                                                                                           |
| `dokumenter`          | array    | ✅         | Minst ett hoveddokument (`hoveddokument=true`)                                                                                                  |
| `tilgangskode`        | string   | 🛡️/valgfri | Brukes ved skjerming                                                                                                                            |
| `tilgangshjemmel`     | string   | 🛡️/valgfri | Skal alltid settes sammen med tilgangskode                                                                                                      |
| `unntattOffentlighet` | array    | 🛡️/valgfri | Settes til `true` dersom avsender skal skjermes. Kan kun settes dersom tilgangskode og hjemmel er satt                                          |
| `person`              | array    | 🛡️/valgfri | Settes til `true` dersom avsender er et personnavn                                                                                              |
| `forsendelsesmetode`  | string   | ✅         | Settes til **GENERELL** dersom Svarut benyttes. <br><br> Settes til **DIG** dersom svar ut **ikke** benyttes.                                   |

## 📮 Mattilsynets praksis for statusløp ved [SvarUt](utsendelse.md)

| Trinn | Statuskode                     | Hva som skjer                                                                            | Utføres av                  |
| :---- | :----------------------------- | :--------------------------------------------------------------------------------------- | :-------------------------- |
| **1** | **R – Reservert**              | Standardstatus ved opprettelse av utgående dokument. Dokumentet er under arbeid.         | Saksbehandler / integrasjon |
| **2** | **F – Ferdig for ekspedering** | Benyttes **kun** dersom dokumentet skal sendes via SvarUt.                               | Integrasjon / system        |
| **3** | **E – Ekspedert**              | Settes automatisk av **SvarUt** når digital forsendelse er gjennomført.                  | SvarUt                      |
| **4** | **J – Journalført**            | Journalposten er ferdigstilt og låst. Settes automatisk via RPA-prosess (kjøres daglig). | RPA (Elements)              |

## 📮 Mattilsynets praksis for statusløp direkte arkivering **uten** forsendelse med SvarUt

| Trinn | Statuskode          | Hva som skjer                                                                                     | Utføres av                  |
| :---- | :------------------ | :------------------------------------------------------------------------------------------------ | :-------------------------- |
| **1** | **R – Reservert**   | Standardstatus ved opprettelse av utgående dokument. Dokumentet er under arbeid.                  | Saksbehandler / integrasjon |
| **2** | **E – Ekspedert**   | Må settes til ekspedert for (indikerer at journalposten er sendt) for å kunne journalføres av RPA | Integrasjon / system        |
| **3** | **J – Journalført** | Journalposten er ferdigstilt og låst. Settes automatisk via RPA-prosess (kjøres daglig).          | RPA (Elements)              |

## 🚨 Vanlige feilmeldinger (API)

- **Manglende mottaker** → «Utgående journalpost krever minst én mottaker».
- **Manglende hoveddokument** → «Journalposttype U må ha minst ett hoveddokument».
- **Forsøk på avskriving** → «Utgående journalposter kan ikke avskrives».
