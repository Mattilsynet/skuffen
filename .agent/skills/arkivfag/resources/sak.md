# Sak

Denne siden gir en oversikt over saksstatuser slik de brukes i Mattilsynets arkivløsning. Formålet er å forklare hva de ulike statuskodene betyr, og hvilke operasjoner som er tillatt i hver status – spesielt med tanke på integrasjoner og bruk av Sikri-API. 

### **Hva er en sak?**

En **sak** er en **samlemappe** i arkivet som inneholder én eller flere journalposter.

- **Saken er rammen**: Den har metadata som tittel, status, ansvarlig enhet og saksbehandler.

- **Journalposter legges på saken**: Alle inngående, utgående og interne dokumenter knyttes til en sak.

- **Dokumenter knyttes til journalposter**: Filene (PDF, DOCX osv.) ligger alltid under journalposten, ikke direkte på saken.

For utviklere betyr dette at:

- En sak må opprettes før man kan opprette journalposter via API.

- Saken bestemmer hvor journalposten «bor» i arkivet.  
  
**Saksstatuser - og hva er tillatt i de ulike statusene?**  

En **saksstatus** beskriver hvor i livsløpet en sak befinner seg, og styrer hvilke operasjoner som er tillatt på saken (f.eks. om den kan redigeres, få nye journalposter eller avsluttes).

Saksstatus

| **Kode** | **Status** | **Tillatt å legge til journalposter?** | **Kommentar** |
| --- | --- | --- | --- |
| **B** | Under behandling | ✅ Ja | Saken er under behandling. Metadata på sak kan redigeres, og man kan legge til journalposter. Når en sak lages via Integrasjon/API til Elements, skal den få status **B (under behandling)** ved opprettelse.  |
| **F** | Ferdig | ✅ Ja | Saken er ferdig behandlet, og kan avsluttes. Metadata på sak kan redigeres, og man kan legge til journalposter.  |
| **A** | Avsluttet | ❌ Nei | Saken er avsluttet ("låst"). Metadata kan ikke redigeres, og man kan ikke legge til journalposter.
Dersom en sak skal endres til **A (avsluttet)** må alle inngående journalposter på saken være journalført og avskrevet, og alle utgående dokumenter må være ekspedert og journalført.  |

### **Felter ved opprettelse av sak**

```
"sakstittel": "string",
  "arkivdel": "string",
  "journalenhet": "string",
  "saksbehandler": "string",
  "saksbehandlerEnhet": "string",
  "saksstatus": "string",
  "ordningsverdi": "string",
  "tilgangskode": "string",
  "tilgangshjemmel": "string",
  "virksomhetsmappeId": "string"
```

Verdier og beskrivelser av de ulike feltene

| **Felter** | **Forklaring** |
| --- | --- |
| Sakstittel | Tittel på saken som skal opprettes i Elements. Se skriveregler i bunnen av dokumentet. |
| Arkivdel | En definert del av et arkiv. I hovedsak brukes verdien **SAK** for tilsynsdivisjonene, mens **SAKHK** brukes for til hovedkontoret.  |
| Journalenhet | En organisatorisk enhet som står for registrering (journalføring). I Mattilsynet skal denne fylles ut med verdien **DOKSENTER** |
| Saksbehandler | Settes hvis saken skal registreres direkte på en saksbehandler. La stå blank hvis det skal legges til fordeling på enhet. |
| SaksbehandlerEnhet | Settes dersom saken skal opprettes på en enhet til fordeling. Fylles ut automatisk hvis saksbehandler er satt.  |
| Saksstatus | Status på sak. Skal settes til **B (under behanding)** ved opprettelse og **A (avsluttet)** når saken er ferdig. |
| Ordningsverdi | Basert på Mattilsynets arkivnøkkel. | Hvis saken skal unntas offentlighet må det påføres tilgangskode med verdien **UO** (Unntatt offentlighet).Tilgangskode krever at man oppgir hjemmel for unntak fra offentlighet. Vær oppmerksom på at journalpostene i saken må påføres tilgangskode og hjemmel etter en individuell vurdering, dvs at hvis man påfører tilgangskode og hjemmel på saksnivå, så betyr ikke det at journalpostene er unntatt offentlighet. |
| Lovhjemmel | Dersom man har påført tilgangskode, kreves det at lovhjemmel oppgis. Eksempelvis Offl. § 23 tredje ledd |
| VirksomhetsmappeId | La denne være blank dersom det ikke er relevant. |
| Kildesystem | Settes til navn på fagsystemet som oppretter saken. |

### ✍️ Skriveregler

Sakstitler skal være beskrivende for innholdet i saken, og meningsbærende med tanke på senere søk og gjenfinning. Følgende skriveregler er retningsgivende:

- Navn på virksomhet/privatperson skal skrives først i sakstittel.

- Etter navn på virksomhet/privatperson skal det skrives en beskrivende og entydig tittel for innholdet i saken.

- Tekststrenger skal skilles med bindestrek (-). Husk mellomrom mellom tekst og bindestrek.

- [Personnavn skal merkes](merke_personnavn.md)

- [Taushetsbelagte opplysninger skal skjermes](merke_personnavn.md) (nøytrale titler som begrenser behovet for skjerming, bør etterstrebes) 

- Unngå:

	- Punktum, komma, kolon, semikolon, parenteser og lignende
	
	
	- Forkortelser
