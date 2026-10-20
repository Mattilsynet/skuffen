# SKU-0024. En mottatt kommando er Skuffens ansvar til den er fullført eller forkastet

Date: 2026-10-12
Last-reviewed: 2026-10-12
Tier: B
Status: Accepted
Crates: domain, application, infrastructure, skuffen-integration-tests

## Related

References: SKU-0016, SKU-0023

## Context

`AvsluttSak` ventet bare på operasjoner. En kommando avvist i validering har
ingen operasjoner, så saken kunne lukkes uten den. Samtidig må en operatør
kunne si at en kommando ikke skal gjennomføres, ellers holder den saken åpen
for alltid.

## Decision

R1 [5]: En kommando Skuffen har sagt OK til, er Skuffens ansvar til den er
  fullført eller forkastet. Hvor behandlingen stoppet er uten betydning.

R2 [5]: `AvsluttSak` venter på alle andre kommandoer på saken, også andre
  `AvsluttSak`. Fullført betyr minst én operasjon og alle `ok`. En dobbel
  avslutning ryddes gjennom admin, ikke i eksekveringslogikken.

R3 [5]: En operatør forkaster kommandoer på
  `arkiv.admin.write.forkast.commands` med `utfort_av` og `command_ids`.
  Ukjent kommando avviser hele forespørselen. Fullført kommando røres ikke.

R4 [5]: Skuffen gjør ikke mer arbeid med en forkastet kommando: den
  dekomponeres ikke, executoren plukker ikke operasjonene dens, retry per
  sak tar den ikke med, og retry med command-id avvises. Ingenting
  reverseres. Forkasting er alltid tillatt; operatøren tar ansvaret.

R5 [5]: Forkasting er endelig. Skal arbeidet gjøres likevel, sender klienten
  en ny kommando.

## Consequences

En avvist kommando holder saken åpen til den er retryet til fullføring eller
forkastet. Hvem som forkastet står i loggen; årsak lagres ikke, og forkasting
kan ikke angres. Klienten får ingen statusmelding om forkastingen; den har
allerede fått feilen som stoppet kommandoen.
