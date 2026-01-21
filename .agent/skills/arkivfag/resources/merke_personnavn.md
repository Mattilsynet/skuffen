# Merke personnavn og skjerme opplysninger

### **Merking av personnavn**

For å ivareta personvernet skal personnavn i sakstittel, journalposttittel og adressekort alltid merkes (tagges) som personnavn, ref. [Forskrift til offentleglova](https://lovdata.no/dokument/SF/forskrift/2008-10-17-1119/%C2%A76#%C2%A76). Dette gjør vi for at personnavnet ikke skal være søkbart på [eInnsyn](https://www.einnsyn.no/) (offentlig journal) i mer enn ett år etter publisering, noe som reduserer muligheten for oppbygging av personprofiler.

For å merke personnavn i **tittelfelt** benyttes tegnet **|**. Eks: **|**_Ola Norrmann_**|**_ - Søknad om godtgjørelse for…_

For å merke personnavn på **adressekortet** på journalposten må verdien `"person": true,` settes ved opprettelse av journalposten.

### **Skjerming av opplysninger i sakstittel, journalposttittel og adressekort**

Skjerming på saker og journalposter skal kun benyttes når det er hjemmel for å unnta dette fra offentligheten. Det betyr at man kun kan skjerme opplysninger dersom sak og/eller journalpost er påført tilgangskode og lovhjemmel.

For å skjerme opplysninger i **tittelfelt** benyttes **[].** Eks: **[**_Ola Norrmann_**]**_ - Søknad om godtgjørelse for…_

For å skjerme avsender/mottaker på **adressekortet** på journalposten må verdien `"unntattOffentlighet": true`, settes ved opprettelse av journalposten.


| **Sak/journalposttittel**                                         | **Forklaring**                                    | **Kommentar**                                        |
| :---------------------------------------------------------------- | :------------------------------------------------ | :--------------------------------------------------- |
| **[\|Ola Norrmann\|] - Søknad om …**                              | Merking av **personnavn og skjerming** i tittel   | Saken/journalposten må ha tilgangskode og lovhjemmel |
| **\|Ola Norrmann\| - Søknad om …**                                | Kun merking av **personnavn** i tittel            |                                                      |
| **Vedtak etter endt saksbehandling - \[Tekst som skal skjermes]** | Kun skjerming av **tekst** i tittel.              | Saken/journalposten må ha tilgangskode og lovhjemmel |
| `"person": true`                                                  | Merking av personnavn på adressekort              |                                                      |
| `"unntattOffentlighet": true`                                     | Skjerming av navn på adressekort                  | Journalposten må ha tilgangskode og lovhjemmel       |
| `"person": true`<br>`"unntattOffentlighet": true`                 | Merking og skjerming av personnavn på adressekort | Journalposten må ha tilgangskode og lovhjemmel       |
