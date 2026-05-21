# SKU-0006. Renderer port, adapter og lagring

Date: 2026-05-13
Last-reviewed: 2026-05-21
Tier: C
Status: Accepted
Crates: application, infrastructure

## Related

References: SKU-0005

## Context

HTML-template dokumenter trenger runtime-integrasjon som gjør substituert HTML om til PDF uten å bygge rendering inn i domain eller application use cases.

Render-resultatet må være replay- og retry-trygt, og operatører må forstå provenance uten å reverse-engineere UUID-er.

## Decision

R1 [7]: Application skal eksponere en `DokumentRenderer` port, og infrastructure skal implementere `Html2PdfRendererAdapter` mot den eksterne Cloud Run-tjenesten.

R2 [7]: Tester skal bruke `FakeDokumentRenderer` med deterministisk success og scriptbare feilsekvenser, ikke ekte Cloud Run-kall i unit tests.

R3 [7]: Renderer-adapteren skal mappe timeouts, network errors, HTTP 5xx og HTTP 401/403 til recoverable feil, mens øvrige 4xx-feil blir sanitized irrecoverable feil.

R4 [8]: Rendered PDF skal lagres i samme NATS object store med deterministisk UUID v5 basert på Skuffen dokument-id.

R5 [8]: NATS object metadata skal inneholde provenance som origin, source template reference, source document id, source command id og render timestamp, ikke brukerfritekst.

R6 [8]: Template-objekter ryddes av bucket TTL, ikke eksplisitt delete i render-handleren, fordi malen er single-use innenfor Skuffens eierskap.

R7 [8]: Tracing og logging skal aldri inneholde HTML body, PDF bytes, substituert `saksnummer`, OIDC-token eller `Authorization` header.

## Consequences

Renderer-integrasjonen holder domain og application fri for HTTP- og Cloud Run-detaljer. Fake-renderer gjør feilscenarioer testbare uten ekstern tjeneste.

Deterministisk UUID v5 gjør retry idempotent. Metadata, ikke UUID-en, forklarer rendered PDFs opphav. TTL-cleanup aksepteres for v1.
