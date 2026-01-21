# Inngående

# 📥 Journalposttype I – Inngående saksdokumenter

Inngående journalposter brukes for dokumentasjon som kommer **utenfra Mattilsynet** (brev, e-post, skjema, høringssvar osv.). Korrespondanse og eventuell dokumentasjon skal arkiveres, og deretter saksbehandles (vurdere behov for oppfølging/svar).

---

## 🧩 Nøkkelpunkter

- **Journalposttype =** `"I"`
- Opprettes **direkte i** `"J"` (journalført)
- Bruk `avskrivDirekte = true`
- Bruk alltid `avskrivningsmaate = "TE"`
- Minst **én avsender** (`erMottaker=false`)
- Minst **ett hoveddokument** (`hoveddokument=true`)

## 🛠️ Obligatoriske felter

| Felt                  | Type     | Påkrevd?   | Kommentar                                                                                              |
| :-------------------- | :------- | :--------- | :----------------------------------------------------------------------------------------------------- |
| `tittel`              | string   | ✅         | Inngående journalposttittel                                                                            |
| `dokumentDato`        | datetime | ✅         | Dato dokumentet er mottatt/registrert                                                                  |
| `journalposttype`     | string   | ✅         | Alltid `"I"`                                                                                           |
| `journalstatus`       | string   | ✅         | Settes til `"J"`                                                                                       |
| `avskrivDirekte`      | boolean  | ✅         | Alltid `true` i vår praksis                                                                            |
| `avskrivningsmaate`   | string   | ✅         | Alltid `"TE"`                                                                                          |
| `saksbehandler`       | string   | ✅         | ID for ansvarlig, bruk ansattnummer                                                                    |
| `saksbehandlerEnhet`  | string   | ✅         | Enhet/organisatorisk tilhørighet. Eksempelvis M34600                                                   |
| `avsendereMottakere`  | array    | ✅         | Minst én **avsender** (`erMottaker=false`)                                                             |
| `dokumenter`          | array    | ✅         | Minst ett dokument med `hoveddokument=true`                                                            |
| `tilgangskode`        | string   | 🛡️/valgfri | Fylles ut hvis saken skal unntas offentlighet. Krever lovhjemmel (tilgangshjemmel)                     |
| `tilgangshjemmel`     | string   | 🛡️/valgfri | Skal alltid settes sammen med `tilgangskode`                                                           |
| `unntattOffentlighet` | array    | 🛡️/valgfri | Settes til `true` dersom avsender skal skjermes. Kan kun settes dersom tilgangskode og hjemmel er satt |
| `person`              | array    | 🛡️/valgfri | Settes til `true` dersom avsender er et personnavn                                                     |

### 📈 Avskriving

**Hva er avskriving?**

- Når et inngående dokument er ferdig behandlet, skal det **avskrives** – det betyr at det er håndtert. Dersom dokumentet ikke er avskrevet kaller vi det for en restanse. Alle inngående dokumenter i en sak må være avskrevet for at man skal kunne avslutte saken ved endt saksbehandling.

**Avskrivningskoder:**

- TE: Tatt til etterretning
- TO: Til orientering
- TLF: Besvart pr. telefon

### 🧪 Videreutvikling eller behov for tilpasning

Denne siden beskriver standardoppsettet for registrering av inngående journalposter som skal arkiveres direkte i status **J (journalført)**.
Dersom det er behov for mer komplekse flyter – for eksempel å opprette journalposter som skal redigeres etter registrering, fordeles, eller få vedlegg lagt til i ettertid – kan dette løses via tilpassede integrasjonsflyter.

## 🚨 Vanlige feilmeldinger

- **Manglende hoveddokument** → «Journalposttype I må ha minst ett hoveddokument».
- **Ingen avsender oppgitt** → «Inngående journalpost krever minst én avsender».
