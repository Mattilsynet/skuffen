# Skuffen - Sequence flere saker happy flow

Kilde: `.agent/skills/skuffen_architecture/resources/flows/Skuffen - Sequence flere saker happy flow.svg`

## Hovedflyt (happy path)
- Start batch 1.
- Opprett sak A.
- Opprett journalpost X pa sak A med vedlegg U.
- Avslutt sak A.
- Opprett journalpost Y pa sak B med vedlegg V.
- Valid data? -> JA.
- Valid state? -> JA.
- Legg i ko.
- Plukk en kommando og gor Request til arkiv.
- OK fra arkiv? -> JA.
- Returner OK i resultat.

## Parallelle handlinger i ko
- Opprett sak A.
- Opprett journalpost X pa sak A.
- Legg til vedlegg pa journalpost X.
- Journalfor journalpost X.
- Avskriv journalpost X.
- Avslutt sak A.
- Opprett journalpost Y pa sak B.
- Legg til vedlegg pa journalpost Y.

## Notat
- Diagrammet markerer parallell handtering.
