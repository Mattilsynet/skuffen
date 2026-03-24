# Skuffen - State machine utgaaende med utsending

Kilde: `.agent/guides/architecture/state_machines/Skuffen - State machine utgaaende med utsending.svg`

## Tittel
- Utgaaende med utsending

## Noder
- R
- F
- E
- Journalfoert
- Avskrevet
- Robot Sender ut med SvarUt
- Robot Setter til J etter sending er fullfort

## Overganger
- Opprett journalpost -> R
- Legg til vedlegg -> R
- Ukjent overgang -> F
- Ukjent overgang -> E
- Avskriv -> Avskrevet
- Robot Sender ut med SvarUt -> (videre flyt)
- Robot Setter til J etter sending er fullfort -> Journalfoert

## Tidsmerking
- 1-2 m
- 0.5-1 h

## Notat
- R, F og E er forkortelser i diagrammet og er ikke forklart i SVG.
- Overganger for R/F/E er delvis implicit i SVG uten labels.
