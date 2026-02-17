# Skuffen - Sequence Error flow

Kilde: `.agent/skills/skuffen_architecture/resources/flows/Skuffen - Sequence Error flow.svg`

## Hovedflyt
- Start batch 1.
- Opprett sak A.
- Opprett journalpost X pa sak A med vedlegg U.
- Avslutt sak A.
- Valid data? -> JA.
- Valid state? -> NEI.

## Feil
- Returner feil i resultat.

## Notat
- Diagrammet indikerer en sekvensfeil knyttet til rekkefolge/avhengighet.
