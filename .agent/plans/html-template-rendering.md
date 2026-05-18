# Implementeringsplan: HTML-template dokumenter med server-side rendering

**Opprettet:** 2026-05-13  
**Build-status:** Implementert 2026-05-13; `lib-schemas`-endringen er pushet og Skuffen er pinned
til commit `91e69423239d3e6b496506e9e48699a26896124e` uten lokal path-patch.
**Beslutningsdokumenter:**

- `docs/adr/skuffen/SKU-0004-dokument-dto-og-ekstern-skjemakontrakt.md`
- `docs/adr/skuffen/SKU-0005-html-template-rendering-og-tilstandsmaskin.md`
- `docs/adr/skuffen/SKU-0006-renderer-port-adapter-og-lagring.md`

## Mål

Skuffen skal støtte dokumenter der klienten laster opp en HTML-mal som trenger verdier som først finnes etter at arkivet har opprettet saken, først og fremst `saksnummer`. Klienten skal ikke orkestrere en ny opplasting etter `OpprettSak`; Skuffen skal kunne se at dokumentet venter på et deklarert felt, substituere når feltet finnes, rendre PDF via `html-to-pdf`, og deretter fortsette eksisterende `LeggTilDokument`-flyt.

## Bekreftede beslutninger

- Wire-kontrakten endres i ekstern `lib-schemas`: `Dokument { client_reference, tittel, form: Dokumentform }`.
- `form` er felt-navnet og blir JSON key. Ikke bruk `dokumentform` i wire-typen.
- `Dokumentform::Bytes { dokument_referanse, filtype }` erstatter dagens flate dokumentfelter.
- `Dokumentform::HtmlTemplate { mal_referanse, felter }` beskriver en opplastet HTML-mal og hvilke felter som kreves.
- Bruk norsk domain-vokabular: `Felt` og `felter`, ikke `Binding` eller `bindings`.
- Behold deklarasjonen (`felter`) selv om HTML-en også inneholder token. Eksekveringsmotoren skal kunne vurdere readiness uten å lese malen.
- v1 støtter `Felt::Saksnummer`, som korresponderer med token `{{saksnummer}}`. Det implisitte subjektet er saken dokumentet tilhører.
- Bruk default serde, eksternt taggede enums, og avvis ukjente varianter. JSON-shape skal kunne forutsies mekanisk fra Rust-typen.
- Maler er single-use i Skuffen: én mal, én rendered PDF, én command. TTL rydder opp malobjekter.
- Rendered PDF lagres i samme NATS object store med deterministisk UUID v5 basert på dokument-id. Provenance ligger i object metadata.
- Ingen HTML, PDF-bytes, substituert `saksnummer`, OIDC-token eller `Authorization` header skal logges.
- Template upload/rendering/status failures in `watch-status` fail hard with non-zero exit code.

## Viktige forutsetninger før bygg

- Systemet ble antatt å være pre-produksjon uten live klienter under planleggingen. Hvis dette ikke lenger stemmer, må compatibility-, migrerings- og rollout-strategi avklares før Phase 1/2.
- `lib-schemas` bor i ekstern repo (`Mattilsynet/landdyrtilsyn-libs`) og Skuffen pinner schema-endringen med `rev = "<commit-hash>"` under utvikling.
- ADR-mode gjelder. Før Skuffen-kode endres mot en crate, kjør `cargo run -p adr-fmt -- --context <crate>` og behandle reglene som hard constraints.
- Phase 0 må bekrefte NATS object-store TTL, executor `ack_wait`, og Cloud Run invoker-tilgang for Skuffen service account.

## Åpne build-time valg

Disse er ikke låst som ADR-beslutninger ennå og skal avgjøres tidlig i build:

1. **UUID v5 namespace-verdi:** velg én fast namespace UUID i kode og dokumenter verdien i implementeringsnotat eller ADR-oppdatering hvis teamet ønsker det. Selve regelen er deterministisk UUID v5 over dokument-id.
2. **`DokumentMedTilstand` shape:** anbefalt shape er en enum som speiler kildeformen, f.eks. `DokumentKildeTilstand::Bytes` og `HtmlTemplate { felter, rendered_dokument_referanse }`, fordi state-machine-reglene blir mer eksplisitte enn med flate optional felter. Hvis build velger en flat shape, må valget begrunnes i PR-beskrivelsen og state-machine-testene.
3. **Mixed-list rekkefølge:** anbefalt utgangspunkt er at `RenderDokument` for en template og `LeggTilDokument` for uavhengige byte-søsken følger eksisterende state-machine rekkefølge uten kunstig serialisering. Pin faktisk behavior i integration test.

## Phase 0 — Prerequisites og måling

**Mål:** Bekreft operative forutsetninger før kodeendring.

**Arbeid som kan kjøres parallelt:**

- Verifiser at NATS object-store bucket for media har TTL. Hvis ikke: lag egen terraform/ops-oppgave før feature-build.
- Mål JetStream consumer `ack_wait` for eksekveringsworker mot faktisk bucket og consumer. Den må dekke fetch, validering, substitusjon, render på opptil 60s, upload og persist med margin. Hvis for lav: øk til omtrent 180s.
- Survey alle dagens `Dokument.dokument_referanse` call sites. Minst kjente steder: `command_listener.rs`, Sikri gateway, query mappings, integration-test support og test fixtures.
- Bekreft at `registrer_i_eksekveringssystem` har tilgang til `Dokumentform`-discriminanten ved intake.
- Identifiser Skuffen service account og koordiner least-privilege Cloud Run invoker-tilgang mot `html-to-pdf`.
- Bekreft HTTP-clientvalg for renderer adapter, sannsynligvis `reqwest`, og OIDC ID-token strategi for Cloud Run audience.

**Gate:** Ingen blocker på TTL, ack-wait eller Cloud Run-tilgang. Eskaler hvis en forutsetning ikke lar seg løse trygt.

## Phase 1 — Schema PR i `lib-schemas` og ADR-baseline

**Mål:** Land wire-kontrakten eksternt og ha Skuffen ADR-er klare før intern cutover.

**Ekstern schema-endring:**

- Endre `Dokument` til `client_reference`, `tittel`, `form: Dokumentform`.
- Legg til `Dokumentform::Bytes { dokument_referanse, filtype }`.
- Legg til `Dokumentform::HtmlTemplate { mal_referanse, felter: Vec<Felt> }`.
- Legg til `Felt::Saksnummer` i schema-craten, fordi `Felt` er wire-kontrakt.
- Bruk default serde. Ikke legg på `#[serde(tag = ...)]`, `#[serde(rename = ...)]` eller `#[serde(other)]`.
- Legg til roundtrip- og negative deserialization-tester for begge enum-nivåer.

**Skuffen docs:**

- Hold ADR-ene `SKU-0004`, `SKU-0005` og `SKU-0006` oppdatert når schema-review avdekker presiseringer.
- Kjør `cargo run -p adr-fmt -- --lint` etter ADR-endringer.

**Gate:** Schema PR har stabil commit hash som Skuffen kan pinne. ADR-lint har ingen infrastructure errors.

**Build note 2026-05-13:** Under lokal implementering var `lib-schemas` midlertidig path-patched til
`../landdyrtilsyn-libs/lib-schemas`. Dette er erstattet med pushed commit
`91e69423239d3e6b496506e9e48699a26896124e`.

## Phase 2 — Schema cutover, state machine og persistence

**Mål:** Skuffen kompilerer mot ny schema-commit, state machine kjenner `AvventerRendring`, og renderer dispatch finnes som stub.

**Seriell kjerne:**

- Bump alle `lib-schemas` git dependencies til schema-commit `rev`.
- Legg til `DokumentTilstand::AvventerRendring`.
- Legg til `ArkivOperasjon::RenderDokument { journalpost_id, dokument_id }`. Behold `journalpost_id` hvis handleren trenger parent scope for persistence/wakeup; fjern det hvis `dokument_id` alene er nok etter repository-design.
- Utvid `dokument_tilstand` med `rendered_dokument_referanse UUID NULL` og eventuell kolonne/sidecar for deklarerte `felter`.
- Utvid CHECK constraints for ny dokumenttilstand.
- Oppdater Postgres mapping samtidig med enum og SQL, slik string-koder og constraints matcher.

**Parallelle etterarbeid:**

- Migrer alle `dokument_referanse` call sites til `match dokument.form`.
- Intake: `Bytes` validerer eksisterende `dokument_referanse`; `HtmlTemplate` validerer `mal_referanse` som object existence/ownership, ikke-tom `felter`, og ingen duplikate `Felt`. Ikke parse HTML ved intake.
- Registrering: `Bytes` starter som `IkkeRealisert`; `HtmlTemplate` starter som `AvventerRendring`.
- Legg til ren domain-modul for token scanning, token/felter-validering og substitusjon.
- Plasser `RenderDokument`-regelen etter permanent-feil gate og før journalføring. v1 readiness: alle deklarerte felter er resolvable, altså `Felt::Saksnummer` krever `sak.saksnummer.is_some()`.
- Legg til dispatch-stub som returnerer klar irrecoverable feil hvis `RenderDokument` faktisk kjøres før Phase 3.

**Tester:**

- Unit-tester for state-machine-readiness, blokkering uten `saksnummer`, permanent feil som vinner over rendering, og journalføring som venter på rendered hoveddokument.
- Pure-domain tester for `{{saksnummer}}`, manglende token, ekstra token, duplikate tokens og store inputs.
- Migrasjonstest: up, down, up.

**Gate:** `cargo test --workspace --exclude skuffen-integration-tests`, `cargo fmt --check`, `cargo clippy --all-targets --all-features`.

## Phase 3 — Renderer port, adapter, handler og intake-validering

**Mål:** Full render-pipeline fungerer med porter, adaptere, fake og safe logging.

**Parallelle byggesteg:**

- Application port `DokumentRenderer` med `async fn render(&self, html: &[u8]) -> Result<Vec<u8>, RendererFeil>`.
- Typed `RendererFeil` som mapper tydelig til recoverable eller irrecoverable execution feil.
- Infrastructure adapter `Html2PdfRendererAdapter` som poster HTML til Cloud Run `html-to-pdf` med connect timeout rundt 5s og total timeout rundt 60s.
- OIDC ID-token acquisition for Cloud Run audience med caching og refresh ved expiry. Auth-feil (401/403) skal gi operatør-actionable, sanitized recoverable feil uten token/header-verdi.
- `FakeDokumentRenderer` for tester, med deterministisk success og scriptbare feilsekvenser.
- Intake-tester for manglende `mal_referanse` og duplikate `felter`.

**Serielle steg etter port/fake:**

- Implementer `render_dokument` handler:
  1. idempotency check for allerede `Ok` med `rendered_dokument_referanse`
  2. defensiv felt-oppløsning fra snapshot
  3. fetch HTML-mal fra NATS object store
  4. scan og valider tokens mot `felter`
  5. substituer token-verdier
  6. beregn deterministic rendered UUID v5
  7. rendr via `DokumentRenderer`
  8. upload PDF til NATS object store med provenance metadata uten brukerfritekst eller substituerte feltverdier
  9. persist `rendered_dokument_referanse` og overgang `AvventerRendring -> Ok` med `tilstand_historikk`
  10. trigger `VentendeKommandoWakeup` for saken
- Bytt Phase 2-stub til ekte dispatch.
- Wire adapter/fake inn i service builder og testsupport.

**Feilsemantikk:**

- Template fetch, renderer timeout, HTTP 5xx og upload failure er recoverable.
- HTTP 4xx fra renderer er irrecoverable, bortsett fra 401/403. 401/403 skal klassifiseres som recoverable auth-/configuration-feil med operatør-actionable, sanitized melding.
- Token/felter mismatch er irrecoverable og setter dokument til `FeiletPermanent`.

**Safe logging tester:**

- Captured tracing skal vise at HTML body, PDF bytes, OIDC token, Authorization header og substituert `saksnummer` aldri logges.

**Gate:** Workspace tester uten integration package, clippy, fmt og ADR context for berørte crates.

## Phase 4 — Integration tests med real NATS/Postgres og fake renderer

**Mål:** Bekreft at pipeline komponerer end-to-end.

**Tester:**

- Positiv lifecycle: `OpprettSak` etterfulgt av journalpost med HtmlTemplate dokument. Assert rendered PDF i NATS, metadata, `rendered_dokument_referanse`, `Ok` dokument og journalført journalpost.
- Negativ mismatch: deklarasjon og HTML-token matcher ikke, dokument blir `FeiletPermanent`, command feiler terminalt.
- Recoverable renderer: fake feiler én gang, command går til retry, neste attempt lykkes.
- Manglende `mal_referanse`: command avvises ved intake.
- Mixed-list test: HtmlTemplate hoveddokument + byte vedlegg, med eksplisitt assertion av faktisk operasjonssekvens.
- Ad-hoc test: `skuffen-manual send-sequence` sends both `Bytes` documents and one `HtmlTemplate`
  document by default. Use `watch-status` to monitor template upload/rendering/status with a
  300s default timeout (configurable via `--timeout-seconds`).

**Gate:** `cargo test -p skuffen-integration-tests` mot lokal stack.

## Phase 5 — Finalisering

**Mål:** Pin merged schema, aksepter ADR-er og avslutt docs.

**Arbeid:**

- Merge `lib-schemas` PR og bump Skuffen fra draft commit til merged commit eller release tag.
- Oppdater ADR-status fra `Proposed` til `Accepted` hvis implementasjonen lander slik dokumentert.
- Hvis implementation avviker, oppdater ADR-ene før status endres.
- Oppdater relevante guider hvis operator workflow eller observability-praksis endres.
- Kjør full verifikasjon: fmt, clippy, workspace tests og integration tests.

## Verifikasjon og review-rytme

- Etter hver meningsfulle phase: kjør `lead-reviewer` med krav om build-gate review før neste phase.
- Ved Rust-implementering: last `rust-coding-style` skill før kodeendring.
- Ved event-/NATS-kontraktendringer: last `nats-event-contracts` skill.
- Ved logging/tracing-endringer: last `tracing-and-safe-logging` skill.
- Dokumentasjon er del av done. Ikke vent til Phase 5 hvis en implementation-beslutning gjør plan eller ADR stale.
