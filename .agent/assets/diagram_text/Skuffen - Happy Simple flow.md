# Skuffen - Happy Simple flow

Kilde: `.agent/guides/architecture/flows/Skuffen - Happy Simple flow.svg`

## Hovedflyt (happy path)
- Start batch 1.
- Opprett sak A.
- Opprett journalpost X pa sak A med vedlegg U.
- Avslutt sak A.
- Valid data? -> JA.
- Valid state? -> JA.
- Legg i ko.
- Plukk en kommando og gor Request til arkiv.
- OK fra arkiv? -> JA.
- Returner OK i resultat.

## Parallelle handlinger i ko
- Opprett sak.
- Opprett journalpost.
- Legg til vedlegg pa journalpost.
- Journalfor journalpost X.
- Avskriv journalpost X.
- Avslutt sak.

## Notat
- Diagrammet markerer parallell handtering med "Parallel".
