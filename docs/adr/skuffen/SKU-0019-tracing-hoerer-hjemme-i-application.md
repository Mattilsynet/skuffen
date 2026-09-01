# SKU-0019. Tracing hører hjemme i application

Date: 2026-08-31
Last-reviewed: 2026-08-31
Tier: B
Status: Accepted
Crates: application, domain, infrastructure

## Related

References: SKU-0016, SKU-0013

## Context

Regelen har vært at `tracing` ikke skal forekomme i `domain` eller
`application`. Den ble skrevet for meldingsdrevet flyt, der infrastruktur eier
grensen for en arbeidsenhet.

SKU-0016 gjør at eksekveringen ikke er meldingsdrevet. Meldingen er acket, og
operasjoner plukkes fra Postgres. Grensen for ett forsøk er kjent i
`EksekverOperasjonService` og ingen andre steder.

En port som lot infrastruktur ramme inn forsøket fungerte, men så bare inn og
ut: `attempt_no` var ukjent, håndterte feil returnerte `Ok(())`, og blokkerte
og pollende operasjoner passerte uten spor.

Bekymringen bak den opprinnelige regelen var lekkasje av teknologi inn i
kjernen. `tracing` er en fasade som er no-op uten subscriber, ikke I/O. Den
reelle risikoen er at `#[instrument]` uten `skip_all` registrerer alle
argumenter via `Debug` — og application er laget der kommandopayloads lever.

## Decision

R1 [5]: `application` kan bruke `tracing`. `domain` kan ikke, og forblir fritt
  for alt annet enn forretningsregler.

R2 [5]: `#[instrument]` i `application` skal alltid ha `skip_all`. Felter velges
  eksplisitt. Makroen skal aldri fange argumenter automatisk.

R3 [5]: Loggpolicyen i observability-guiden gjelder uendret: ingen request
  payloads til eksterne systemer på `info!` eller `error!`, og PII fra domene-
  og kommandotyper kun på `debug!`.

R4 [6]: Spans som rammer inn orkestrering lages der grensen er kjent.
  Eksekveringen instrumenteres i tjenesten selv, ikke gjennom en port.

R5 [5]: Observability skal ikke ha egne porter i application. Porter er for I/O
  og sideeffekter, ikke for å rute et tverrgående anliggende utenom en lagregel.

## Consequences

Application kan si hva den gjør der den gjør det. Blokkeringsårsak, forsøksnummer,
poll-frist og feilkode blir synlige uten å fraktes ut gjennom et grensesnitt.

`skip_all` er den mekaniske garantien. Den håndheves i CI, slik at et glemt
`skip_all` stoppes før payloaden når loggen, ikke etterpå.

Grensen mot `domain` blir viktigere fordi den er den eneste absolutte igjen.
Til gjengjeld er den enklere å håndheve: `domain/Cargo.toml` har ingen
tracing-avhengighet.

Vi aksepterer at «ingen teknologi i indre lag» ikke lenger er en enkel regel,
men to: `domain` er rent, `application` orkestrerer og kan fortelle om det.
