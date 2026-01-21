---
name: Arkivfag
description: Domeneregler og retningslinjer for håndtering av arkivobjekter (Sak, Journalpost, Dokument) i Skuffen.
---

# Arkivfag

Skuffen skal være et enklere interface for utviklere i Mattilsynet. Denne kunnskapen beskriver hvordan Mattilsynet vil at arkivering skal foregå mot det bakenforliggende arkivsystemet.

Detaljert dokumentasjon for domenereglene ligger i `resources/`-mappen. Du skal bruke disse filene som oppslagsverk.

## Oversikt over ressurser

*   **[Sak](resources/sak.md)**
    *   Inneholder regler for livssyklus (B -> F -> A), statuskoder og krav til metadata ved opprettelse.
    
*   **[Journalpost](resources/journalpost)**
    *   Mappe som inneholder definisjoner for `Inngående`, `Utgående` og `Internt Notat`.
    
*   **[Dokument](resources/dokument.md)**
    *   Beskriver sammenhengen mellom journalpost og dokument, krav til titler og filformater.

*   **[Personvern og Skjerming](resources/merke_personnavn.md)**
    *   **Viktig**: Inneholder syntaksreglene for merking av personnavn (`|Navn|`) og skjerming av taushetsbelagt informasjon (`[Info]`).

## Instruksjoner for bruk

Når du skriver kode eller planlegger funksjonalitet i Skuffen:

1.  **Slå opp reglene**: Ikke gjett på statuskoder eller feltnavn. Les den relevante filen i `resources/`.
2.  **Personvern**: Sjekk alltid `merke_personnavn.md` hvis du håndterer titler eller navn.
