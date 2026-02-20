---
name: Skuffen Process
description: Obligatoriske arbeidsflytregler for oppgaveutførelse, risikovurdering og planlegging.
---

# Skuffen Prosess & Styring (Governance)

Denne ferdigheten (skill) definerer den obligatoriske arbeidsflyten for all utvikling i Skuffen.

## Kjerne-regler (Core Rules)

1.  **Strukturert Utvikling & Kvalitet**
    - **Mål:** Vi skal ha full kontroll og forståelse for alt som skjer i systemet. Dette er et produksjonssystem.
    - **Fleksibilitet:** Oppgaver kan komme fra `.agent/governance/tasks.md` ELLER direkte fra chat/dialog.
    - **Krav:** Uansett kilde, SKAL `.agent/governance/tasks.md`, `.agent/governance/plan.md` og `.agent/governance/roadmap.md` oppdateres for å reflektere virkeligheten. Vi bygger strukturert.
    - **Fokus:** Kvalitet fremfor hastighet. Vi skal "skjønne alt".

2.  **Bevissthet om Risiko & Visjon**
    - **Før** du starter implementasjon, MÅ du konsultere `.agent/governance/risk.md`.
    - Sikre at dine planlagte endringer ikke bryter med sikkerhet, dataintegritet eller arkitektoniske risikoer definert der.

3.  **Dynamisk Planlegging & Risikoanalyse**
    - Hvis oppgaven din involverer oppdatering av `.agent/governance/plan.md` eller `.agent/governance/roadmap.md`, **MÅ** du utføre en dyp risikoanalyse.
    - **Trigger:** Plan/Veikart oppdateres -> **Handling:** Gå gjennom påvirkning -> **Output:** Oppdater `.agent/governance/risk.md` med nye funn eller mitigeringer.

## Oppsummering av Arbeidsflyt

1.  Sjekk `.agent/governance/tasks.md` for neste oppgave.
2.  Les `.agent/governance/risk.md` for begrensninger/føringer.
3.  Opprett implementasjonsplan.
4.  (Hvis planen endrer Prosjektplan/Veikart) -> Oppdater `.agent/governance/risk.md`.
5.  Utfør (Execute).

## Diagram-tekster

Se `.agent/assets/diagram_text/INDEX.md` for tekstlige tolkninger av flow- og state machine-diagrammer.
