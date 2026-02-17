# Skuffen - State Sequence Error flow

Kilde: `.agent/skills/skuffen_architecture/resources/flows/Skuffen - State Sequence Error flow.svg`

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
- Valid state? -> NEI.

## Feil
- Returner feil i resultat.
- Kan ikke gjore operasjoner pa avsluttet sak (ligger avslutt sak kommando i koen).

## Tidsmerking
- T = 0, T = 1, T = 2, T = 3, T = 4 (i diagrammet).
