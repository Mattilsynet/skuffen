# Skuffen - State machine journalpost

Kilde: `.agent/guides/architecture/state_machines/Skuffen - State machine journalpost.svg`

## Noder
- S/R/J
- Journalfoert
- Avskrevet

## Overganger
- Opprett sak -> S/R/J
- Opprett journalpost -> S/R/J (selv-loop)
- Legg til vedlegg -> S/R/J (selv-loop)
- Journalfoer -> Journalfoert
- Avskriv -> Avskrevet
