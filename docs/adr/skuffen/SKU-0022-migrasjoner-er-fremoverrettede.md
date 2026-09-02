# SKU-0022. Migrasjoner er fremoverrettede og reversible fra nummer to

Date: 2026-09-02
Last-reviewed: 2026-09-02
Tier: D
Status: Accepted
Crates: infrastructure

## Related

References: SKU-0016

## Context

Repoet har hatt én migrasjon, initialskjemaet, med en `.down.sql` som er
`DROP TABLE` på alt. Så lenge databasen bare inneholdt testdata var det
uproblematisk — rollback betydde å begynne på nytt.

Med produksjonssetting endres regnestykket. Innholdet i `operasjon`,
`sak_tilstand` og `entitet` er arkivfaglig sporbarhet: hvilken kommando
materialiserte hvilken fact, hvilket forsøk gikk mot arkivet, og hva utfallet
ble. Det finnes ikke andre steder. En rollback som dropper tabellene er ikke en
rollback, det er datatap.

Samtidig er en down-migrasjon reell verdi for endringer som *kan* reverseres:
en ny kolonne, en utvidet indeks, en løftet constraint. Der er den billig å
skrive og verdifull å ha når en utrulling må trekkes tilbake.

`sqlx::migrate!` kjører aldri down-migrasjoner via `run()`; kun `revert()` gjør
det, og den kalles ingen steder i Skuffen. Fraværet av en `.down.sql` er derfor
uten konsekvens for normal drift.

## Decision

R1 [10]: Initialmigrasjonen har ingen `.down.sql`. Skjemaet som bærer
  produksjonsdata skal ikke kunne droppes av et verktøy.

R2 [10]: Migrasjon to og senere skal ha `.down.sql` som gjenoppretter skjemaet
  uten å miste data. Backfilte verdier som blir stående etter en reversering er
  akseptabelt; slettede rader eller kolonner med innhold er det ikke.

R3 [9]: En down-migrasjon skal aldri slette data som ikke ble innført av den
  tilhørende up-migrasjonen. Feiler den heller enn å slette, er det riktig
  utfall.

R4 [9]: Migrasjoner kjøres av applikasjonen ved oppstart, serialisert av sqlx'
  advisory lock. Ingen manuell DDL mot produksjon utenom en migrasjonsfil.

## Consequences

Rullebakk av skjemaendringer er mulig for alt unntatt selve grunnfjellet. Det er
den riktige asymmetrien: initialskjemaet endres ikke igjen, mens hver senere
endring er liten nok til å reverseres.

En feilende down-migrasjon er et akseptabelt utfall og ikke en bug. Alternativet
er et verktøy som stilltiende sletter arkivsporbarhet fordi noen kjørte feil
kommando.

R2 skiller bevisst mellom skjema og data. En backfill av typen
`UPDATE ... SET kolonne = now() WHERE kolonne IS NULL` kan ikke reverseres —
hvilke rader som var `NULL` er ikke lenger kjent. Det er likevel en ren
reversering i denne ADR-ens forstand: skjemaet gjenopprettes, ingen rad
forsvinner, og verdien som blir stående er gyldig. Kravet er datasikkerhet, ikke
bit-for-bit symmetri.

Vi aksepterer at expand/contract ikke er formalisert her. Med én instans i Cloud
Run er det ikke to skjemaversjoner i drift samtidig, så spørsmålet er utsatt,
ikke løst. Skalerer tjenesten horisontalt, må denne ADR-en utvides før neste
skjemaendring.
