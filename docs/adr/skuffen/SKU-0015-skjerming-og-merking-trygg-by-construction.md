# SKU-0015. Skjerming og merking trygg by construction

Date: 2026-08-05
Last-reviewed: 2026-08-05
Tier: A
Status: Accepted
Crates: skuffen, domain, application, infrastructure, sikri_client, skuffen-integration-tests

## Related

References: SKU-0004, SKU-0008, SKU-0013 | Supersedes: SKU-0014

## Context

Skjerming (unntatt offentlighet) og korrekt merking av korrespondanseparter og
utsendingsmottakere er sikkerhetskritisk: feil kan eksponere skjermet informasjon
eller sende post til feil mottaker. Dagens wire-kontrakt uttrykker skjerming som
klientstyrt tittel-markup og lar gateway utlede Sikri-merking med fallback-verdier.
Det gjør feil-åpen oppførsel mulig og gjør skjerming representerbar uten hjemmel.

Vi redesigner kontrakten slik at skjerming og merking er trygg by construction:
ulovlige tilstander skal være urepresenterbare i typene, gateway skal feile lukket,
og en post-condition audit skal bekrefte at skjermings-intensjon faktisk ble
materialisert mot Sikri. Redesignen er en koordinert breaking change uten live
klienter (jf SKU-0008), bygger på ekstern serde-tagging fra SKU-0004, og holder
SKU-0013-boundaryen absolutt. Full kodesett-validering utsettes til senere leveranse.

## Decision

R1 [5]: Wire-kontrakten uttrykker skjerming som `Tilgjengelighet { Offentlig, Skjermet { tilgangskode, tilgangshjemmel } }` med eksternt tagget serde (jf SKU-0004 R6). Skjerming uten hjemmel er urepresenterbart; `Skjermet` uten både kode og hjemmel kan ikke konstrueres eller deserialiseres.

R2 [5]: Korrespondansepart har eksplisitt `parttype` (Person/Virksomhet) som gir Sikri `person`-merking. Utsendingsmottaker bærer `MottakerId` (`Person { fødselsnummer }` / `Virksomhet { organisasjonsnummer }`) og full `Postadresse` (SvarUt GENERELL). «Kun digital»/ikke-SvarUt er utenfor v1; kontakt arkivutvikling for det.

R3 [5]: Nye newtypes `Tilgangskode`, `Tilgangshjemmel` og `Postnummer` er non-empty-validerte nå. Full kodesett-validering (TILGANGSKODE/TILGANGSHJEMMEL mot autoritative kodelister) er en senere leveranse og skal ikke antas utført av v1-typene.

R4 [5]: Infrastructure/gateway utleder ALLE Sikri-felt (`person`, `unntattOffentlighet`, `erMottaker`, `forsendelsesmetode=GENERELL`, `tilgangskode`/`hjemmel`, `id`/`id_type`). Gateway skal ALDRI feile åpent: ingen `unwrap_or_default`/`None`-fallback for skjerming eller merking; manglende eller tvetydige felt avvises som irrecoverable.

R5 [5]: Post-condition audit: før Sikri-suksess aksepteres verifiseres at `Skjermet`-intensjon faktisk ga `unntattOffentlighet=true` og ikke-tom kode/hjemmel på utgående DTO. Deretter emittes et trygt audit-event (correlation id, journalpost id, `shielded: bool`) UTEN navn eller fødselsnummer.

R6 [5]: Fri tittel eies av klient. Fri-tekstfelt (journalpost-tittel mot journalpostens `Tilgjengelighet`, sakstittel mot sakens `Tilgjengelighet`) som inneholder skjermings-markup `[ ]` krever `Skjermet`, ellers avvises. Balansert-brakett med escape `\[` for legitime `[sic]`/`[1]` defineres; korrespondansepart-navn skal være markup-fritt.

R7 [5]: Query-svar bruker EGNE permissive respons-typer som kun rapporterer tilstand og aldri re-validerer, slik at historiske koder alltid kan deserialiseres. Skuffen maskerer ikke query-svar; klienten er en intern trusted konsument med tjenstlig behov.

R8 [4]: SKU-0013 R1-R4 er absolutt: wire-typer skal ALDRI importeres i `domain`/`application`; infrastructure oversetter ved laggrensen. Den nye shapen tar ingen snarveier rundt denne boundaryen.

R9 [4]: Rå HTTP-response-tekst fra Sikri (og lignende eksterne response-bodies med uforutsigbart innhold) logges KUN på `debug!`-nivå, aldri på `info!`/`error!`; på høyere nivåer brukes saniterte error-koder. Debug er av i prod som default. PII i strukturerte kommando-/wire-typer kan vises via `Debug` fordi det bare treffer `debug!`-nivå; det som ikke skal blø ut på `info!`/`error!` er rå ekstern response-tekst.

## Consequences

- Skjerming uten hjemmel og merking uten mottaker-id blir umulig å uttrykke;
  feilklasser flyttes fra runtime til compile-time og til eksplisitt avvisning.
- Gateway feiler lukket, og post-condition audit gir et bevis-spor på at
  skjermings-intensjon faktisk ble materialisert mot Sikri. Audit-eventet bruker
  trygge felt (correlation id, journalpost id, shielded-flagg), ikke PII.
- Restrisiko som ikke kan lukkes av typer dokumenteres eksplisitt:
  under-klassifisering (klient velger `Offentlig` for reelt sensitiv data) og
  tittel-omission (skjermet innhold uten markup) reduseres av audit og
  prod-deteksjon, men elimineres ikke.
- Redesignen er breaking mot eksisterende wire-shape; koordineres via SKU-0008
  uten live klienter. Full kodesett-validering gjenstår som senere leveranse.
- «Kun digital»/ikke-SvarUt-utsending er bevisst utenfor v1.
