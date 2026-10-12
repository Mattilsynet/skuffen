# SKU-0023. Admin retry slipper stoppet arbeid inn i ordinær behandling

Date: 2026-09-28
Last-reviewed: 2026-09-30
Tier: B
Status: Accepted
Crates: domain, application, infrastructure, skuffen-integration-tests

## Related

References: SKU-0016, SKU-0017, SKU-0018, SKU-0020

## Context

En kommando som avvises i validering, eller en operasjon som feiler terminalt,
kan ha en årsak som senere rettes utenfor Skuffen — for eksempel en sak som ble
avsluttet i arkivet og så gjenåpnet. Vanlig gjenutsendelse hjelper ikke:
mottaket dedupliserer på `command.dispatchet_at` (SKU-0016 R11), og klienten får
en `Ok`-kvittering uten at noe skjer. Eneste utvei var manuell SQL og sletting
av klientens statusspeil.

Skuffen er allerede idempotent i hvert steg: identiteter gjenbrukes ved replay,
dekomponering setter inn null rader andre gang, og executoren avgjør selv hva
som kan utføres ut fra fakta. Et nytt forsøk trenger derfor ikke en egen
behandlingsmodell, bare en kontrollert måte å slippe arbeidet inn igjen på.

## Decision

R1 [5]: Admin retry har to eksakte subjects: `arkiv.admin.write.retry.commands`
  tar en eksplisitt liste `command_ids`; `arkiv.admin.write.retry.sak` tar en
  saksnøkkel `key`. Hvert subject godtar bare sin request-type, uten alias eller
  wildcard-subscription. Saksnøkkelen har samme format som admin read:
  `skuffenId`, `clientReference` eller `arkivId`.
  Command-id-er, entitets-id-er, arkiv-id-er og
  forsøkshistorikk beholdes; det finnes ingen behandlingsrunder og ingen ny
  command-id.

R2 [5]: En kommando uten operasjoner og uten dekomponering revalideres med
  originalen fra inbox-strømmen gjennom den ordinære validatoren. Hvorfor den
  stoppet lagres ikke og vurderes ikke.

R3 [5]: Eksisterende operasjoner frigis etter domeneregler: `feilet` blir `klar`,
  `retry_venter` og `blokkert` fremskyndes. `klar`, `kjorer`, `sendt` og `ok`
  røres ikke; executoren avgjør utfallet som vanlig.

R4 [5]: Uavklart utfall (`krever_avklaring`) på en berørt sak avviser hele
  forespørselen før frigivelse og revalidering, også når operasjonen tilhører
  en kommando utenfor listen. Planlegging kan lagre manglende sakskoblinger.

R5 [6]: Admin retry gjør ingen egne arkivoppslag, fakta-refresh eller arkivskriv.
  Validering og executor beholder sine vanlige kontroller, og samme feil på nytt
  er et gyldig utfall.

R6 [6]: Frigivelsen låser sakens operasjonsrader med `FOR UPDATE NOWAIT` i én
  transaksjon før kontroll og oppdatering. En konflikt med en annen transaksjons
  radlås avviser frigivelsen uten endringer, med vanlig `Error` og meldingen
  «Saken har operasjoner som utføres. Prøv igjen senere». Det innføres ingen
  «opptatt»-state. Status `kjorer` eller `sendt` alene utløser ikke feilen.

R7 [5]: `command.sak_id` er sakens interne `skuffen_id`, ikke payloadens
  saksreferanse. Mottaket setter koblingen etter identitetsregistrering når
  referansen kan løses til en lokal sak. Dekomponering fyller bare en eventuell
  `NULL`-verdi med sakens ID fra planen; en eksisterende kobling beholdes.
  Saksutvalg kan dermed finne tilknyttede kommandoer uten å lese payloaden,
  også når valideringen avviste dem før dekomponering.

R8 [5]: Admin retry er et eksplisitt unntak fra monoton terminal feil (SKU-0016
  R8): en frigitt kommando kan publisere nye statuser etter `Avvist` eller
  `Feilet`, og klienter må tåle det.

R9 [5]: Revalidering publiserer ready-meldingen med unik meldings-id, og
  dekomponering av en allerede dekomponert kommando skriver ingenting. En replay
  kan dermed ikke overskrive nyere fakta.

R10 [5]: `utfort_av` er påkrevd, selvdeklarert attribusjon og logges på `info!`
  sammen med command-id-ene. Skrivetilgang gis per subject, separat fra admin
  read, som forblir read-only (SKU-0018).

R11 [5]: Saksutvalget leses fra ett lokalt snapshot via `command.sak_id`, med
  operasjonenes sakskobling som fallback bare når kolonnen er `NULL`. Alle
  tilknyttede kommandoer inngår, sortert på mottakstid og command-id, og bruker
  samme retry-flyt som en eksplisitt liste. Nye kommandoer etter oppslaget inngår ikke.

R12 [5]: Maks 100 unike kommandoer tillates, også ved saksutvalg. Større utvalg
  avvises før arbeid slippes inn, uten trunkering eller batching. Ukjent lokal
  sak gir `Sak not found`; kjent sak uten tilknyttede kommandoer kvitteres uten arbeid.

R13 [5]: Admin logger forsøket og overlater behandlingsutfall til validator,
  executor og ordinær statusstrøm. Vellykket kall gir `NatsResponse::Ok(())`,
  uten resultatliste eller tellere. Teknisk feil stopper videre revalidering
  og gir sanitert feilsvar; normale valideringsutfall tolkes ikke av admin.

## Consequences

Drift kan rette en årsak i arkivet og be om et nytt forsøk uten SQL, uten nye
id-er og uten at utført arbeid gjentas. Handlingen er sporbar gjennom
attribusjonsloggen og de ordinære milepælsloggene for hver kommando.

Radlåsene holder operasjonsradene stabile mens frigivelsen kontrollerer
sakssperren og endrer retry-tilstanden. Uten `NOWAIT` kan admin holde rad B og
vente på rad A, mens executor holder A og venter på B. PostgreSQL må da avbryte
en transaksjon. Hvis executorens transaksjon avbrytes etter et vellykket
arkivskriv, rulles ikke arkiveffekten tilbake sammen med databasen. `NOWAIT`
lar admin gi seg ved radlåskonflikten og unngår denne ventesyklusen; det er
ikke en generell garanti mot alle vranglåser.

Alle kommandoer angir hvilken sak de gjelder: `OpprettSak` gir en ny
`client_reference`, mens øvrige kommandoer refererer til en sak. Dette betyr
ikke at enhver innsendt referanse kan løses til en lokal identitet. Mottaket
registrerer identiteten for `OpprettSak` og for en `ArkivId`-referanse før
koblingen settes, men oppretter ikke en sak for en ukjent `client_reference`.
Da kan `command.sak_id` være `NULL`. For nye kommandoer med en oppløsbar
referanse er koblingen allerede komplett ved mottak; dekomponeringens
`COALESCE(sak_id, ...)` er utfylling av manglende kobling, ikke en nødvendig
andre fase for alle kommandoer. Den dekker også eldre mottaksrader som ennå
ikke er dekomponert.

Terminal er ikke lenger evig: `Avvist` og `Feilet` betyr at utfallet var avgjort
da det ble publisert, og at bare en eksplisitt admin-handling kan åpne det
igjen. Klienter som speiler status må godta senere hendelser etter et terminalt
utfall og følge den siste. `krever_avklaring` er fortsatt en sperre: et ukjent
skriveutfall må avklares av et menneske før noe på saken prøves igjen.

Originalen må finnes i inbox-strømmen, som har 180 dagers maksimal alder. Eldre
avviste kommandoer kan ikke revalideres, og forespørselen avvises tydelig.

Saksoppslaget finner ikke historiske avvisninger uten både `command.sak_id` og
operasjoner. Disse må fortsatt adresseres med command-id; admin gjør ikke et
globalt søk i inbox. Kjent tomt saksutvalg beviser derfor ikke at saken aldri
har hatt avviste kommandoer. Fullførte kommandoer teller med i grensen på 100,
men utføres ikke på nytt.

Et direkte validator-kall har ingen NATS-levering rundt seg. Et midlertidig
valideringsutfall gir derfor ikke automatisk nytt forsøk eller statusevent;
årsaken finnes i validatorens logg. Kvitteringen bekrefter admin-handlingen,
ikke godkjent validering, fremdrift eller fullført arkivering.

Teknisk feil eller manglende svar utelukker ikke sideeffekter. Frigivelsen er
allerede committet før revalidering, og dispatch kan ha lyktes før en senere
statuspublisering feiler. Tidligere arbeid rulles ikke tilbake. Listens
rekkefølge er behandlingsrekkefølge, ikke en utføringsgaranti.
