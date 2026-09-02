# SKU-0020. Ett beslutningssted for operasjonsutfall

Date: 2026-09-02
Last-reviewed: 2026-09-02
Tier: C
Status: Accepted
Crates: application, infrastructure, skuffen-integration-tests

## Related

References: SKU-0016, SKU-0017

## Context

SKU-0016 innførte et periodisk evalueringspass som flytter `blokkert → klar`.
Passet og executoren kaller samme `vurder`, leser samme fakta og skriver samme
tilstand. Bare executoren publiserer status.

Operasjoner fødes `blokkert` fordi det er kolonnedefaulten, så hver eneste
operasjon passerer passet før den kan kjøres. Er første beslutning
`AlleredeUtfort` eller `Ugyldig`, skrives operasjonen terminal uten ett eneste
event. `OpprettSak`, `AvsluttSak` og `SettSaksansvarlig` dekomponerer til én
operasjon hver, så for dem blir hele kommandoen stille for alltid.

Årsaken er ikke en glemt publisering. `blokkert`/`klar` er en materialisert
cache av en ren funksjons output, med to skrivere som har ulike sideeffekter.
Leseren stoler ikke engang på cachen: `execute` regner beslutningen ut på nytt
med en gang den plukker raden.

Passet hentet blokkerte med `ORDER BY created_at LIMIT 200`. Systemet
akkumulerer permanent blokkerte rader ved design, og de er de eldste. Passerer
antallet grensen, brenner passet hele budsjettet på de samme døde radene og
frigjør aldri noe nytt.

Statusstrømmen hadde `Nats-Msg-Id`-deduplisering nøklet på `attempt_no`. Fire
hendelser bruker `attempt_no = 0`, så de kolliderte. `duplicate_window` var
usatt, så serverdefaulten på to minutter gjaldt uansett.

## Decision

R1 [6]: Kun `EksekverOperasjonService` avgjør en operasjons utfall. Ingen annen
  kodesti skriver terminal status eller blokkering etter en beslutning.
  Beslutning og publisering hører sammen i én kodesti.

R2 [7]: Kjørbarhet er en forfallsklokke, ikke en statuscache. `neste_forsok_at`
  er `NOT NULL DEFAULT now()`, og workeren plukker `klar`, `retry_venter` og
  `blokkert` som har forfalt.

R3 [7]: En blokkert operasjon sjekkes på nytt med fast frekvens, 30 sekunder, i
  stedet for kontinuerlig. Frekvensen bounder kostnaden ved permanent blokkerte
  rader uten egen status, egen teller og uten manuell opplåsing.

R4 [7]: Når en operasjon fullfører, settes blokkerte søsken på samme sak
  forfalt. Hendelsesdrevet vekking er et latenshint; forfallsklokken er
  mekanismen som garanterer fremdrift.

R5 [8]: Statusstrømmen er at-least-once og deduplisert av ingen. Klienten må
  tåle duplikater, særlig gjentatt `Feilet` når flere operasjoner på samme
  kommando feiler terminalt.

R6 [7]: Varsling om `krever_avklaring` markeres i databasen, ikke utledet av
  hvor mange rader recovery flyttet. En krasj mellom commit og publisering skal
  ikke etterlate stille rader.

## Consequences

`EvaluerOperasjonerService` utgår. Worker-løkka blir ett kall mot executoren, og
den doble beslutningsstien kan ikke gjeninnføres ved uhell fordi det bare finnes
ett sted å skrive utfall.

Utsultingen forsvinner uten ny status. `ORDER BY neste_forsok_at` roterer
gjennom alle rader, og en permanent blokkert operasjon blir kjørbar av seg selv
i det årsaken forsvinner — ingen opplåsingsmekanisme å vedlikeholde.

Frekvensen er fast og ikke eskalerende. En eskalerende trapp ville krevd en egen
teller, fordi `attempt_no` kun inkrementeres rett før et arkivkall og derfor står
på null for en blokkert operasjon. Ved dagens volum er en fast sjekk hvert
halvminutt billigere enn kolonnen den ville kostet.

Prisen er at søskenvekkingen gjeninnfører hendelsesdrevet oppvåkning, som
SKU-0016 forkastet. Forskjellen er at den nå er et hint oppå en pålitelig timer,
ikke eneste mekanisme. Uteblir vekkingen, blir fremdriften treg, ikke borte.

Blokkerte operasjoner deler den serielle executor-løkka med ekte arkivkall.
Ved dagens volum er det uten betydning; ved høyere volum er det forfallsklokken
og jitter som må justeres først.

At-least-once er nå sant i kontrakten og ikke bare i praksis. Dedupliseringen ga
en illusjon om exactly-once innenfor et udokumentert tominuttersvindu, og
kostet en nøkkel koblet til `attempt_no`.
