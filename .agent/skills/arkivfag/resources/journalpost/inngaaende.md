# Inngående

# 📥 Journalposttype I – Inngående saksdokumenter

Inngående journalposter brukes for dokumentasjon som kommer **utenfra Mattilsynet** (brev, e-post, skjema, høringssvar osv.). Korrespondanse og eventuell dokumentasjon skal arkiveres, og deretter saksbehandles (vurdere behov for oppfølging/svar).

---

## 🧩 Nøkkelpunkter

- **Journalposttype =** `"I"`
- `journalstatus` settes **ikke** ved opprettelse — Elements åpner journalposten
  i en status der endringer er mulige
- `avskrivDirekte` og `avskrivningsmaate` settes **ikke** ved opprettelse
- Journalføring (`J`) og avskriving (`TE`) er **egne steg etterpå**
- Minst **én avsender** (`erMottaker=false`)
- Minst **ett hoveddokument** (`hoveddokument=true`)

> ⚠️ En journalført journalpost er **låst**. Opprettes den direkte i `J`, kan
> vedlegg ikke legges til i ettertid. Skuffen oppretter derfor journalposten
> åpen, legger på vedlegg, og journalfører til slutt.

## 🛠️ Obligatoriske felter

| Felt                  | Type     | Påkrevd?   | Kommentar                                                                                              |
| :-------------------- | :------- | :--------- | :----------------------------------------------------------------------------------------------------- |
| `tittel`              | string   | ✅         | Inngående journalposttittel                                                                            |
| `dokumentDato`        | datetime | ✅         | Dato dokumentet er mottatt/registrert                                                                  |
| `journalposttype`     | string   | ✅         | Alltid `"I"`                                                                                           |
| `journalstatus`       | string   | ⛔         | Settes **ikke** ved opprettelse. Journalføres i eget steg når alle dokumenter er på plass              |
| `avskrivDirekte`      | boolean  | ⛔         | Settes **ikke** ved opprettelse. Avskriving er et eget steg                                            |
| `avskrivningsmaate`   | string   | ⛔         | Settes **ikke** ved opprettelse. Alltid `"TE"` når avskriving utføres                                  |
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

### 📮 Statusløp

| Trinn | Hva som skjer                                                       | Utføres av           |
| :---- | :------------------------------------------------------------------ | :------------------- |
| **1** | Journalposten opprettes med hoveddokument. `journalstatus` settes ikke | Integrasjon / system |
| **2** | Vedlegg legges til, ett om gangen                                    | Integrasjon / system |
| **3** | **J – Journalført.** Journalposten låses og publiseres til eInnsyn   | Integrasjon / system |
| **4** | **TE – Avskrevet.** Dokumentet er tatt til etterretning              | Integrasjon / system |

Rekkefølgen er ikke valgfri: vedlegg må være på plass før journalføring, og
avskriving forutsetter at journalposten er journalført.

## 🚨 Vanlige feilmeldinger

- **Manglende hoveddokument** → «Journalposttype I må ha minst ett hoveddokument».
- **Ingen avsender oppgitt** → «Inngående journalpost krever minst én avsender».
