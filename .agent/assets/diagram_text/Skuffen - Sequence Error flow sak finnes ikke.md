# Skuffen - Sequence Error flow sak finnes ikke

Kilde: `.agent/guides/architecture/flows/Skuffen - Sequence Error flow sak finnes ikke.svg`

## Hovedflyt
- Start batch 1.
- Opprett sak A.
- Opprett journalpost X med vedlegg U pa sak A.
- Avslutt sak A.
- Opprett journalpost Y med vedlegg V pa sak B.
- Valid data? -> JA.
- Valid state? -> NEI.

## Feil
- Returner feil i resultat.
- Sak B finnes ikke.
