# Utsendelse

Mattilsynet benytter [KS sin løsning](https://ksdigital.no/tjenestene/svarut-tjenesten/) SvarUt for ekspedering av utgående saksdokumenter fra Elements Cloud. 

### Hva er SvarUt?
SvarUt er en generell meldingsformidler. Det betyr at SvarUt velger forsendelsesmåte tilpasset den enkelte mottaker via digitale kanaler som Altinn, Digipost og eBoks (eller som vanlig brevpost hvis det ikke er mulig å sende digitalt).

For å ekspedere utgående saksdokumenter digitalt via SvarUt må følgende krav være tilfredsstilt:

| | |
| :--- | :--- |
| **Digital adresse** | For å sende digitalt med SvarUt må forsendelsen være påført *organisasjonsnummer* eller *fødselsnummer*, i tillegg til fullstendig postadresse. Dette gjøres i feltene under *Avsendere/mottakere* på journalposten. Mottaker må ha adresse i Norge.<br><br>**Organisasjonsnummer/fødselsnummer + navn og fullstendig postadresse = <u>den digitale adressen</u>.** |
| **Forsendelsesmåte (metode)** | Riktig forsendelsesmåte for SvarUt er: **GENERELL - Generell digital forsendelse.**<br><br>Forsendelsesmåte (metode) finner du i feltene under *Avsendere/mottakere* på journalposten. |
| **Filformater** | En forsendelse med SvarUt kan ikke inneholde følgende filformater: ZIP-filer, PDF-portefølje, lyd- og videofiler. Hvis det er aktuelt å sende lyd-/videofiler, ta kontakt med team arkivutvikling. |

### **Hva trigger en forsendelse?**
Hvis kriteriene i tabellen over er oppfylt, blir dokumentet ekspedert ved at man endrer status på journalposten fra R - Reservert til F - Ferdig.

Når forsendelsen er ekspedert får journalposten status E-Ekspedert, og kan journalføres (status J).

### Felter under *Avsendere/mottakere* på journalposten (adressekortet)

| Felt | Påkrevd? | Kommentar |
| :--- | :--- | :--- |
| `navn` | ✅ | Navn til avsender/ mottaker |
| `organisasjonsnummer` | ✅ | Organisasjonsnummer eller fødselsnummer til avsender/mottaker |
| `epost` | ⛔ | E-post adresse til avsender/mottaker |
| `telefon` | ⛔ | Telefonnummer til avsender/mottaker |
| `postadresse` | ✅ | Postadresse til avsender/mottaker |
| `postnummer` | ✅ | Postnummer til avsender/mottaker |
| `poststed` | ✅ | Poststed til avsender/mottaker |
| `utlandsadresse` | ⛔ | Utenlandsadresse er ikke et felt på adressekortet i Elements |
| `forsendelsesmetode` | ✅ | Hvis utgående brev, så må det påføres en forsendelsesmetode (måte) for å kunne ekspedere brevet. |
| `erMottaker` | ✅ | Hvis utgående brev: true |
| `kopi` | ⛔ | Hvis kopimottaker: true |
| `unntattOffentlighet` | ⛔ | Hvis avsender/mottaker skal skjermes: true (krever tilgangskode og hjemmel på journalposten) |
| `person` | ⛔ | Hvis personnavn: true |
| `tilSaksbehandler` | ⛔ | Benyttes hvis x-notat med intern mottaker |
| `tilSaksbehandlerEnhet` | ⛔ | Benyttes hvis x-notat med intern mottaker |
