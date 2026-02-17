# Skuffen - General flow

Kilde: `.agent/skills/skuffen_architecture/resources/flows/Skuffen - General flow.svg`

## Hovedflyt (hoyt nivaa)
- Batch av kommandoer kommer inn.
- Valid data?
  - NEI -> Returner feil i resultat.
  - JA -> Valid arkiv state (sak og bruker)?
- Valid arkiv state?
  - NEI -> Returner feil i resultat.
  - JA -> Plukk en kommando og gor request til arkiv.
- OK fra arkiv?
  - JA -> Returner OK i resultat og fjern fra ko.
  - NEI -> Recoverable ERROR: RETRY eller Irrecoverable ERROR.

## Kjoing/ko
- Legg til handlinger i enden av eksisterende ko.

## Beslutninger og labels
- JA/NEI brukes i beslutningsnoder.
- Recoverable ERROR: RETRY er egen gren.
- Irrecoverable ERROR er egen gren.
