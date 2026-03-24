# Skuffen - State Sequence Happy flow

Kilde: `.agent/guides/architecture/flows/Skuffen - State Sequence Happy flow.svg`

## Batch 1
- Start batch 1.
- Opprett sak A.
- Opprett journalpost X med vedlegg U pa sak A.
- Valid data? -> JA.
- Valid state? -> JA.
- Legg i ko.
- Plukk en kommando og gor Request til arkiv.
- OK fra arkiv? -> JA.
- Returner OK respons.

## Batch 2
- Start batch 2.
- Opprett journalpost Y med vedlegg V pa sak A.
- Valid data? -> JA.
- Valid state? -> JA.
- Legg i ko.
- Plukk en kommando og gor Request til arkiv.
- OK fra arkiv? -> JA.
- Returner OK i resultat.

## Ko
- Ko oppdatert med de nye elementene.

## Tidsmerking
- T = 0, T = 1, T = 2, T = 3, T = 4, T = 5 (i diagrammet).
