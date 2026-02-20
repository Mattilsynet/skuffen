# Skuffen - Arkiv sak finnes Happy flow

Kilde: `.agent/skills/skuffen_architecture/resources/flows/Skuffen - Arkiv sak finnes Happy flow.svg`

## Hovedflyt (happy path)
- Start batch 1.
- Opprett journalpost X med vedlegg U pa sak A.
- Avslutt sak A.
- Valid data? -> JA.
- Valid state? -> JA.
- Legg i ko.
- Plukk en kommando og gor Request til arkiv.
- OK fra arkiv? -> JA.
- Returner OK i resultat.

## Parallelle handlinger i ko
- Opprett journalpost.
- Legg til vedlegg pa journalpost.
- Avslutt sak.
