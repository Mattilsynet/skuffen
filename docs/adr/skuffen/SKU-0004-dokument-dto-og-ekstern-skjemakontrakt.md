# SKU-0004. Dokument DTO og ekstern skjemakontrakt

Date: 2026-05-13
Last-reviewed: 2026-05-13
Tier: B
Status: Proposed
Crates: skuffen, domain, application

## Related

References: SKU-0002

## Context

Skuffen trenger en wire-kontrakt som skiller ferdige dokument-bytes fra HTML-maler som rendres etter at Sikri har gitt saken `saksnummer`.

Kontrakten ligger i ekstern `lib-schemas`, må være lesbar uten serde-spesialkunnskap, og skal bruke norsk domain-vokabular: `Dokumentform`, `Felt` og `felter`.

## Decision

R1 [5]: `Dokument` i wire-kontrakten skal ha feltene `client_reference`, `tittel` og `form`, der `form` er JSON-nøkkelen og typen er `Dokumentform`.

R2 [5]: `Dokumentform::Bytes` skal representere ferdig opplastet dokumentinnhold med `dokument_referanse` og `filtype`, og er semantisk fortsettelsen av dagens dokumentmodell.

R3 [5]: `Dokumentform::HtmlTemplate` skal representere en opplastet HTML-mal med `mal_referanse` og `felter`, ikke en ferdig PDF eller et klientstyrt render-resultat.

R4 [5]: `Felt` skal være del av ekstern wire-kontrakt, og v1 støtter `Felt::Saksnummer` som korresponderer med HTML-tokenet `{{saksnummer}}`.

R5 [5]: `felter`-deklarasjonen skal beholdes fordi readiness vurderes fra kommandoens data, mens HTML-tokenene brukes til render-time validering og substitusjon.

R6 [5]: Wire-JSON skal følge default serde med eksternt taggede enums; ukjente `Dokumentform`- og `Felt`-varianter skal avvises uten fallback-variant.

R7 [5]: Variantenes navn og felt skal bruke norsk domain-vokabular, mens Rust- og serde-konvensjoner beholdes som standard teknisk språk.

## Consequences

Dette er et breaking schema-skifte fra dagens flate felter og må koordineres via `lib-schemas` før Skuffen pinner ny commit.

Felt-deklarasjonen dobler delvis HTML-tokeninformasjon, men gjør state-machine-readiness pure og billig. Token/felter-mismatch oppdages som permanent kontraktsfeil under render.
