# Skuffen - Arkiv irrecoverable error

Kilde: `.agent/guides/architecture/flows/Skuffen - Arkiv irrecoverable error.svg`

## Hovedflyt
- Start batch 1.
- Opprett journalpost X med vedlegg U pa sak A.
- Avslutt sak A.
- Valid data? -> JA.
- Valid state? -> JA.
- Legg i ko.
- Plukk en kommando og gor Request til arkiv.
- OK fra arkiv? -> NEI.
- Irrecoverable ERROR.

## Resultat
- Returner feil i resultat.

## Notat
- Diagrammet markerer irrecoverable error som endelig feilgren.
