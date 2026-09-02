# SKU-0017. Terminal feil krever positivt treff

Date: 2026-08-25
Last-reviewed: 2026-09-02
Tier: C
Status: Accepted
Crates: sikri_client, domain, application, infrastructure

## Related

References: SKU-0016, SKU-0015

## Context

SKU-0016 R6 sier at kun irrecoverable feil gir terminal `feilet`. Regelen var
ikke implementert. `sikri_client` klassifiserte riktig, men bar klassifiseringen
i en markørstreng som ingen leste: executoren mappet hvert arkivkall gjennom en
`recoverable()`-hjelper, og valideringen forsøkte å downcaste en strengbasert
`anyhow` til `reqwest::Error`, noe som aldri kunne treffe. Alt fra arkivet ble
retryet for alltid. Et ukjent saksnummer ga verken `Validert` eller `Avvist` —
bare stillhet og en varm løkke mot arkivet.

Klassifiseringen hadde i tillegg en fallback der enhver ukjent 4xx ble
irrecoverable. Sikri autentiseres med brukernavn og passord fra Secret Manager
uten token-refresh. Med den fallbacken ville et rotert passord terminert hver
operasjon som var underveis, og hver av dem ville publisert `Feilet`, som per
SKU-0016 R8 er monotont og aldri kan trekkes tilbake. Én autentiseringshendelse
kunne permanent drept alt i flukt, uten mekanisme for å gjenåpne noe.

## Decision

R1 [6]: Terminal `feilet` krever positivt treff i et eksplisitt regelsett. Bunnen
i klassifiseringen er `Recoverable`. En ukjent feil retryes til noen legger inn
en regel for den.

R2 [6]: Statuskoder klassifiseres eksplisitt begge veier. `401` og `403` er
recoverable fordi de oftest betyr rotert credential, ikke ugyldig forespørsel.
`404` er irrecoverable. Regelsettet utvides etter hvert som feil observeres.

R3 [6]: Body-regler evalueres før statusregler, slik at et positivt treff på
kjent feiltekst terminerer selv når statuskoden alene er recoverable.

R4 [5]: `sikri_client` eier klassifisering, stabil kode og klientvendt melding, og
eksponerer dem som en typet feil. Ingen konsument utleder klassifisering fra
strenginnhold.

R5 [5]: Adapterne i `infrastructure` oversetter den typede feilen til domenetyper
og legger på klientvendt feilkode. Application videreformidler klassifisering,
melding og feilkode uten å tolke dem.

R6 [8]: Feilens felter har hver sin mottaker. `kode` og `intern_detalj` går til
`operasjon.siste_detalj`; `melding` og `error_code` går til klienten. Underliggende
feiltekst når aldri statusstrømmen.

R7 [7]: Tester som låser klassifisering skal gå gjennom kallveien, ikke kalle
mappingfunksjonen direkte. Isolerte mappingtester var grønne mens kallveien var
brutt.

R8 [6]: Prinsippet gjelder også dekomponering. En dekomponeringsfeil er transient
og NAK-es med eskalerende forsinkelse, med mindre den treffer et eksplisitt
regelsett for permanente feil — da ackes den terminalt og `Feilet` publiseres til
klienten.

## Consequences

Feil som faktisk er irrecoverable blir nåbare for første gang. Operasjoner som i
dag henger usynlig i `retry_venter` går terminalt, og klienter som har sett
stillhet begynner å se `Feilet` og `Avvist`. Det er forbedringen, men det er en
atferdsendring klientene bør varsles om.

Klienten får den faktiske grunnen i stedet for én generisk streng. Meldingene
navngir leverandøren — «Sikri/Elements», «ePhorte» — fordi de sier presist hva
som må rettes, og den presisjonen er mer verdt enn å skjule hvilket arkiv som
ligger bak.

Prisen for R1 er at en ekte, ukartlagt klientfeil retryes for alltid i stedet for
å stoppe. Det er en bevisst avveining: å retrye er billig og reversibelt, mens å
terminere er permanent. Motvekten er at `sikri_*`-kodene i `siste_detalj` gjør
slike tilfeller synlige, slik at regelsettet kan utvides.

`404` er irrecoverable både for validering, der det betyr ukjent saksnummer, og
for `AvventJournalfort`, der det kan bety en journalpost som ennå ikke er synlig.
Valget er låst i test. Begynner polling å terminere, er det den regelen som skal
revurderes — ikke bunnen i R1.

R8 lukker det samme hullet ett lag lenger ut. Dekomponeringen NAK-et enhver feil,
også `entitet type mismatch` og `for mange dokumenter`, som aldri kan bli bedre av
å prøve igjen. Klienten hadde da fått `Validert` og ventet på et utfall som aldri
kom, mens meldingen sirkulerte til `max_age`. Uten `max_deliver` og uten DLQ er en
terminal sti i koden eneste vei ut.
