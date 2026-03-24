# Skuffen - Legg til vedlegg paa ikke ferdig journalpost ERROR

Kilde: `.agent/guides/architecture/flows/Skuffen - Legg til vedlegg paa ikke ferdig journalpost ERROR.svg`

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

## Batch 2 (ny request pa samme sak)
- Start batch 2.
- Legg til vedlegg pa journalpost X.
- Valid data? -> JA.
- Valid state? -> NEI.

## Feil
- Returner feil i resultat.
- Kan ikke gjore operasjoner pa journalfort journalpost (ligger journalfor/avskriv kommando i koen).

## Tidsmerking
- T = 0, T = 1, T = 2, T = 3, T = 4 (i diagrammet).
