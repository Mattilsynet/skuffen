# SKU-0010. Registration queue status og terminal lifecycle ownership

Date: 2026-05-29
Last-reviewed: 2026-05-29
Tier: B
Status: Accepted
Crates: skuffen, domain, application, infrastructure

## Related

References: SKU-0007, SKU-0001

## Context

Registration beregner initial kø-status fra `CommandStateDecision`. En kandidat-implementasjon mappet `Done`/`Invalid` direkte til `ok`/`feil` ved registrering — det ville hoppet over executor-flow, attempts og outward-publisering. SKU-0007 R3 tetter ikke dette hullet eksplisitt.

## Decision

R1 [5]: Registrering eier ikke terminal lifecycle. Registration-tjenesten skal aldri skrive `ok` eller `feil` til `command_execution`, uansett utfall fra `CommandStateDecision`.

R2 [5]: Executor eier alle `ok`- og `feil`-overganger. Kun executor-path (command attempt completion) publiserer terminal command lifecycle, inkludert outward status-events.

R3 [5]: Når registration beregner `CommandStateDecision::Done` eller `CommandStateDecision::Invalid` for en ny kommando, skal begge mappas til `klar`. Executor fastslår terminalt utfall med tilgang til attempt-logg, diagnostikk og outward-publisering.

R4 [6]: Wake-up beholder rett til å terminalisere `blokkert_venter`-kommandoer som evaluerer til `Invalid`, i tråd med SKU-0001 R6. Wake-up har allerede verifisert at videre arbeid er umulig — en annen livssyklus-situasjon enn ny-registrering.

## Consequences

- `RegistrerIEksekveringssystemService` produserer kun `klar` eller `blokkert_venter`; den skriver aldri `ok`/`feil`.
- Executor er eneste sted for terminal status, noe som sikrer konsistent outward-publisering og attempt-diagnostikk.
- `Done`/`Invalid` ved registrering medfører én ekstra executor-runde — akseptabel overhead.
- Wake-up-terminalisering (SKU-0001 R6) er et navngitt unntak og forblir uberørt.
