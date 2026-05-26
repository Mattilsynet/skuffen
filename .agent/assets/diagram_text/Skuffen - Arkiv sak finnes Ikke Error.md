# Skuffen - Arkiv sak finnes Ikke Error

Kilde: `.agent/guides/architecture/flows/Skuffen - Arkiv sak finnes Ikke Error.svg`

## Hovedflyt
- Start batch 1.
- Opprett journalpost X med vedlegg U pa sak A.
- Avslutt sak A.
- Valid data? -> JA.
- Valid state? -> NEI.

## Feil
- Returner feil i resultat.
- Feilmelding: Saken det refereres til finnes ikke.

## Notat
- Diagrammet viser en tidsmerking (T = 4) ved feilgrenen.
- Feilen oppstar nar ArkivId Sak **ikke** eksisterer i Sikri eller er stengt.
- Hvis saken eksisterer og er apen, passerer validering og registrering seedes lokal tilstand.
