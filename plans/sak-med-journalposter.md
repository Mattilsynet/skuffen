# Plan: saksoppslag med journalposter og dokumentmetadata

Status: Implementert. Operatørens avklaringer ved implementasjon:

- `dokument_dato` er `chrono::NaiveDateTime`, serialisert uten tidssone.
- Hoveddokument som ikke kan fastslås entydig gir en enkel, kontrollert feil.
  Et dokument regnes som hoveddokument når det er markert som det, eller når ID-en
  er journalpostens `hoveddokId`; nøyaktig ett må treffe.
- Ingen størrelsesgrense mot NATS utover en liten kontroll: query-svar over 8 MB
  gir `Response too large`.
- `lib-schemas` følger `master` uten tag inntil operatøren tagger en release.

## 1. Mål og avtalte beslutninger

En klient skal kunne hente en sak med journalposter og dokumentmetadata gjennom
Skuffen, også når saken og barna ikke er opprettet eller registrert lokalt av
Skuffen. Dokumentinnhold hentes ikke.

| Tema | Beslutning |
|---|---|
| Nytt subject | `arkiv.request.sak.med_journalposter` |
| Request | Saksnøkkel som i eksisterende `HentSakQuery`: `arkivId` eller `clientReference`. Ingen inkluderingsparameter. |
| Nytt oppslag | Ber alltid Sikri om journalposter; dokumentmetadata følger hver journalpost. |
| Eksisterende oppslag | `arkiv.request.sak.hent` beholder oppførselen uten inkludering av journalposter. Direkte saksnummeroppslag rettes også her. |
| Saksnummer | `arkivId` går direkte til arkivet, uten krav om lokal Skuffen-ID. |
| Klientreferanse | `clientReference` slås opp lokalt for å finne saksnummer; referansen må tilhøre en sak. |
| Dokumentrekkefølge | Første dokument i Skuffens liste er hoveddokumentet. Resten er vedlegg. Ingen nytt offentlig hoveddokumentflagg. |
| Dokumentidentitet | Arkivets `dokument_id` skal være med på hvert dokument. |
| Dokumentdato | Obligatorisk, typet datetime i intern lesemodell og offentlig respons; ikke `String`. |
| Saksbehandler på journalpost | Valgfri. Journalposten skal kunne leses uten tildelt saksbehandler. |
| Respons | Gjenbruk eksisterende `SakResponse` / `JournalpostResponse`-struktur, med nødvendige felt-/typeendringer. Ingen V2-subject. |
| Sideeffekter | Kun lesing. Ingen lokal seeding, import, registrering eller endring i arkivet. |

Request-eksempel med syntetisk saksnummer:

```json
{
  "key": {
    "type": "arkivId",
    "value": "2026/12345"
  }
}
```

Svar bruker eksisterende `NatsResponse<SakResponse>`-ramme.

### Ikke i scope

- Filinnhold, base64, dokumentnedlasting eller nye URL-løfter.
- Ferdigbehandling, avskriving, journalføring, admin eller command-endringer.
- Databaseendringer og lokal materialisering av eksterne arkivobjekter.
- Generell omlegging av alle query-felt til permissive typer eller ukjente
  enumvarianter. Et behov utover avtalte felt tas tilbake til operatøren.
- Endring av den kanoniske arkivfaglige kunnskapen.

## 2. Verifisert grunnlag

Operatøren har levert faktiske Sikri-svar for samme sak med og uten inkludering:

- Uten inkludering er `journalposter` tom selv om `antallJournalposter` er syv.
- Med inkludering følger syv journalposter og deres `dokumenterRespons`,
  inkludert en journalpost med både hoveddokument og vedlegg.
- Dokumentene har dokument-ID, hoveddokumentmarkering, tittel og filtype.
- `dokumenter` er `null`; read-mapping skal bruke `dokumenterRespons`.
- `dokumentBase64` er `null` i eksemplene. Dette er ikke en generell garanti for
  alle Sikri-svar; Skuffens respons skal uansett ikke eksponere innholdsfeltet.
- Ufordelt saksbehandler er representert som `"---"` med enhet `"[Ufordelt]"`.
  Det er ikke et eksempel på faktisk JSON-null, men viser at tildelt behandler
  ikke er nødvendig.
- Dokumentdato returneres som dato og klokkeslett uten offset, selv om input
  hadde UTC-markør. Verken inputens klokkeslett eller tidssone kan antas bevart.

Dette underbygger ett `HentArkivsak`-kall med `inkluderJournalposter=true`.
Ingen N+1-kall per journalpost skal innføres uten konkret behov.

Eksemplene inneholder miljø- og personopplysninger og kopieres ikke til repoet.
Testfixtures skal gjenskape struktur og relevante verdier med syntetiske data.

### Dagens kode og mangler

- `src/infrastructure/src/query/nats/listener.rs` hardkoder inkludering til
  `false`. Service, repository og Sikri-klient har allerede et internt bool-flagg.
- `query/mapping/fra_dto_til_domene/sak.rs` krever lokal Skuffen-ID for arkiv-ID.
  Repositoryet oversetter den tilbake til saksnummer før Sikri-kallet.
- `query/mapping/fra_sikri_til_domene/sak.rs` gjør også lokal ID-lookup på
  returen, og `fra_domene_til_dto/sak.rs` slår opp saksnummer igjen.
  Request-retting alene fjerner derfor ikke avhengigheten til lokal registrering.
- Dokumentmapping krever dokument-ID, men kaster den. Hoveddokumentmarkering
  forsvinner i Sikri-klientens dokumentmodell; journalposten beholder `hoveddok_id`.
- Intern journalpostmodell krever saksbehandler og har dokumentdato som `String`.
  Wire-responsen har allerede optional saksbehandler, men også dato som `String`.
- Manglende barnelister blir tomme lister. Saksresponsen settes alltid til
  `Some(...)`, selv når journalposter ikke ble etterspurt.
- Application- og infrastrukturfakes ignorerer inkluderingsflagget. Eksisterende
  NATS-test oppretter lokal sak først og sjekker bare at responsen er `Ok`.

## 3. Kontrakt og datarepresentasjon

### 3.1 Dokument-ID og hoveddokument først

Utvid `DokumentResponse` med obligatorisk `dokument_id`, fortrinnsvis den
eksisterende offentlige `DokumentId`-typen i `lib-schemas`. Denne er en string-
newtype for arkividentitet, ikke Skuffens lokale UUID. Intern lesemodell bruker
egen intern type; den skal ikke importere wire-typen.

Behold `dokument_referanse` med eksisterende betydning. Ikke generer en UUID
eller fyll dette feltet med arkivets dokument-ID.

Sorter dokumenter ved Sikri-boundary før metadata om rolle fjernes:

1. Identifiser hoveddokumentet fra Sikris `hoveddokId` og dokumentmarkering.
2. Flytt det til indeks 0, med stabil rekkefølge for resten av dokumentene.
3. Returner alle dokumenter med ID, tittel og filtype, uten offentlig rolleflagg.

En eksplisitt tom dokumentliste kan forbli tom. En ikke-tom liste hvor
hoveddokumentet mangler, flere er markert eller ID og markering motsier hverandre,
kan ikke sannferdig følge kontrakten ved å gjette. Anbefalt regel er kontrollert
feil uten delvis respons; bekreft denne avviksregelen før implementasjon.

### 3.2 Obligatorisk datetime

`dokument_dato` skal være en datetime-type i både `domain::model::journalpost`
og `lib_schemas::skuffen::query::responses::JournalpostResponse`.

JSON har ingen egen datetime-type. Verdien vil derfor fortsatt serialiseres som
tekst, men Rust-typen og deserialiseringen skal sikre at den er en gyldig dato
med klokkeslett. Manglende eller ugyldig verdi er feil; ingen tom streng,
dagens dato, dato-only-fallback eller annen default.

**Avklares før schema-release: tidssonesemantikk.** Eksempelvis
`2026-02-03T12:30:00` inneholder ingen offset og kan ikke uten videre bli et
`DateTime<Utc>`.

- Anbefalt representasjon ut fra observert sonefri respons er
  `chrono::NaiveDateTime`, hvis dokumentdato er arkivets dato/klokkeslett uten
  tidssone. Da bevares verdien uten å finne på et absolutt tidspunkt.
- Hvis kontrakten skal uttrykke et absolutt tidspunkt, må Sikris tidssone og
  håndtering av sommertid avklares. Velg da `DateTime<FixedOffset>` eller
  `DateTime<Utc>` med eksplisitt, dokumentert konvertering.
- Ikke legg til `Z`, bruk maskinens lokale tidssone eller fjern en mottatt
  offset stilltiende. Avklar også hvordan eventuelle svar med offset behandles.

Chrono med Serde-støtte finnes allerede i berørte crates. Definer og test
aksepterte inputformater, sekundbrøker og eksakt JSON-output når typen velges.
En rå ekstern DTO kan beholde tekst frem til boundary-parsing; ferdig mappet
lesemodell og offentlig respons skal aldri bruke `String` for datoen.

Denne endringen gjelder query-responsen. Command-payloads, admin og lagrede
datofelt skal ikke endres som en skjult følge av arbeidet.

### 3.3 Valgfri saksbehandler

Endre intern journalpostmodell til `Option<String>` og fjern kravet om tildelt
saksbehandler i read-mappingen. Faktisk null skal nå wire-responsen som `None`.

Bevar foreløpig plassholderne `"---"` og `"[Ufordelt]"` som mottatt. Oversettelse
til `None` er en egen normaliseringsregel, ikke implisitt del av optionalitet.
Ikke innfør krav om saksbehandlerenhet. En generell utvidelse av enhets- og
korrespondansepartmapping er ikke nødvendig for denne leveransen.

### 3.4 Journalpostlisten

Behold `SakResponse.journalposter: Option<Vec<JournalpostResponse>>`.

Følgende fraværssemantikk er anbefalt konkretisering av Option-feltet, og skal
bekreftes når kontrakten fryses; den er ikke en separat operatørbeslutning:

- På nytt subject blir en returnert liste `Some(listen)`, også ved eksplisitt `[]`.
- Manglende/null journalpostliste bevares som `None`, ikke oppdiktet tom liste.
- Med eksisterende Serde-attributt utelates feltet ved `None`; det blir ikke
  JSON-null. Klienten må ikke tolke utelatt felt som bekreftet tom sak.
- Ikke endre eksisterende subjects observerte tomliste-shape utilsiktet når
  felles interne modeller endres. Lås gammel oppførsel med regresjonstest.

Dokumentlisten i dagens `JournalpostResponse` er derimot en `Vec`, ikke `Option`.
Ikke innfør en ny null-kontrakt der uten avklaring. Anbefalt håndtering av
manglende `dokumenterRespons` som samtidig motsies av `harHoveddokument` eller
vedleggsantall er kontrollert feil, ikke et misvisende tomt dokumentsett.

### 3.5 Koder og skjerming

Operatørens eksempler bruker støttede typer/statuskoder. Generell støtte for
ukjente koder er ikke besluttet her. Dokumenter dagens begrensning og test at
ugyldig/ukjent kode gir kontrollert feil, uten oppdiktet kjent verdi eller skjult
bortfiltrering av journalposter. Behov for nye koder tas opp separat.

SKU-0015, beslutningen om skjerming og egne permissive query-typer, gir fortsatt
retningen for read-modeller. Denne avgrensede leveransen løser ikke alle kjente
avvik fra prinsippet. Ikke svekk eksisterende fail-closed-håndtering av delvis
skjerming for å få sparse metadata gjennom.

## 4. Implementasjon i rekkefølge

### A. Frys kontrakten og oppdater schema-biblioteket

1. Avklar datetime-valget i §3.2, listesemantikken i §3.4 og avviksreglene for
   dokumentlister. Bekreft også foreslått størrelseshåndtering i steg D.
2. Bekreft adgang før redigering i søskenrepoet `landdyrtilsyn-libs`.
3. Legg til dokument-ID og typet, obligatorisk dokumentdato i eksisterende
   responstyper. Behold saksresponsens struktur og optional saksbehandler.
4. Gjenbruk request-shapen med bare `key`. Velg separat request-type eller
   handler-wrapper for nytt subject, slik at inkluderingsvalget er bundet til
   handleren og ikke en klientstyrt parameter.
5. Legg til schema-tester for JSON-shape, obligatoriske felt og datetime-format.

### B. Fjern lokal identitetsavhengighet fra sakslesing

1. La intern query-nøkkel uttrykke saksnummer eller klientreferanse, og la en
   lookup-only port løse klientreferanse til saksnummer. Arkiv-ID går direkte.
2. La `SakRepository` hente med saksnummer. Ikke bruk lokal Skuffen-ID som
   obligatorisk mellomledd for lesingen.
3. Bær saksnummer i sakens interne lesemodell og map det direkte til responsen.
   Fjern også lokale ID-oppslag på returveien.
4. Behold kontroll av at klientreferansen tilhører en sak, og kontrollert feil
   for ukjent referanse eller referanse uten arkiv-ID. Ingen lokal registrering.
5. La begge saks-subjects bruke den rettede lesestien. Ikke endre command-side
   ID-modeller, seeding eller admin-oppslag.

### C. Før metadata gjennom hele mappingen

1. Bevar dokument-ID og tilstrekkelig hoveddokumentinformasjon i Sikri-klienten
   frem til sortering. Ikke endre felles klientmodeller slik at command-kall
   utilsiktet får nye read-valideringskrav.
2. Parse obligatorisk dokumentdato ved read-boundary til avtalt datetime-type.
3. Bevar optional journalpost-saksbehandler og journalpostlistens fravær.
4. Sorter hoveddokument først og map dokument-ID hele veien til wire-responsen.
5. Ingen innholds- eller URL-kall; ingen base64 eller dokument-URL i ny respons.

### D. Koble opp nytt subject

1. Registrer `arkiv.request.sak.med_journalposter` med inkludering lik `true`;
   eksisterende saks-handler beholder `false`.
2. Oppdater `QueryListener`, bootstrap og tester. Behold queue-group-mønsteret
   og feilpropagering/supervision for langlevde listeners.
3. Bruk samme response-envelope og saniterte feilramme som eksisterende queries.
4. Anbefalt tillegg som bekreftes i steg A: kontroller serialisert responsstørrelse
   mot NATS-grensen før publisering. Returner en avtalt, liten feilrespons
   (foreslått `Response too large`) hvis svaret er for stort. Ikke avkort lister
   eller la dette bare bli en timeout. Hvis tillegget utsettes, dokumenter
   begrensningen og følg opp separat; ikke lov at store svar er håndtert.
5. Avklar subscribe-/publish-rettigheter for nytt subject i driftskonfigurasjon.

## 5. Testplan og akseptansekriterier

Bruk syntetiske fixtures med samme relevante struktur som operatørens bevis.
Testene for listeavvik/fravær og størrelse nedenfor gjelder de foreslåtte
reglene dersom de bekreftes i steg A; juster dem til den endelig avtalte kontrakten.

| Nivå | Hva testen skal bevise |
|---|---|
| Schema | Eksakt request-shape, dokument-ID i svaret, obligatorisk og gyldig datetime, optional saksbehandler, uendret envelope. |
| Datetime | Manglende, null, ugyldig dato og dato-only avvises; gyldig dato/klokkeslett og sekundbrøker roundtripper etter avtalt format. Offset/sonefrie verdier følger eksplisitt policy. |
| Dokumentmapping | Hoveddokument returnert sist fra Sikri flyttes først; vedleggenes relative orden og alle ID-er bevares. Test manglende/duplisert/motstridende hoveddokument etter avtalt regel. |
| Journalpostmapping | Null-saksbehandler og ufordelt-plassholdere fungerer. Obligatorisk dato håndheves. Ukjent kode og delvis skjerming blir kontrollerte feil. |
| Liste-semantikk | Manglende journalpostliste er ikke tom liste på nytt subject. Eksplisitt tom liste returneres som tom. Manglende dokumentliste med observerte dokumenter skjules ikke. |
| HTTP-contract | Gammelt subject fører til eksisterende kall uten inkludering; nytt gir `inkluderJournalposter=true`. Nested dokumenter leses fra `dokumenterRespons`. Ett Sikri-kall per sak. |
| Application | Recording fake viser riktig saksnummer og inkluderingsvalg. Arkiv-ID bruker ikke lokal ID-lookup; client reference gjør det. |
| NATS/E2E | Begge subjects kan hente en arkivsak uten lokal entitet. Nytt svar har flere journalposter, dokument-ID-er, datetime og hoveddokument først. Deserialiser til reelle responstyper, ikke bare `serde_json::Value`. |
| Regresjon | Client-reference-oppslag fungerer fortsatt; feil entitetstype/ukjent referanse avvises. Gammelt saksoppslag inkluderer fortsatt ikke barn. |
| Sikkerhet | Base64 i syntetisk upstream-response finnes ikke i public svar. Ingen metadata/body i ordinære logger. Ingen lokale eller eksterne writes under lesing. |
| Størrelse | For stort serialisert svar gir en liten feilrespons; ingen stille avkorting eller publish-feil som bare blir klient-timeout. |

Fakes må slutte å ignorere inkluderingsvalget. Minst én test må gå gjennom
ekte HTTP-deserialisering og mapping, slik at en rik fake alene ikke skjuler
tap av dokument-ID eller feil datoformat.

Kjør ved implementasjon:

```bash
cargo check
cargo fmt --check
cargo test -p application
cargo test -p sikri_client
cargo test --workspace --exclude skuffen-integration-tests
cargo clippy --all-targets --all-features
cargo test -p skuffen-integration-tests
```

Integrasjonstestene krever NATS og Postgres; se `.agent/guides/commands.md`.
Kjør også schema-cratets relevante tester i bibliotekrepoet.

## 6. Kompatibilitet, dokumentasjon og utrulling

Nytt subject isolerer inkluderingsvalget, men **ikke alle schema-endringene**:
`JournalpostResponse` og `DokumentResponse` er delte typer.

- String til datetime endrer Rust-API og aksepterte tekstformater. JSON er
  fortsatt tekst, men en ny typed klient kan avvise et gammelt svar.
- Obligatorisk dokument-ID gjør gamle dokumentresponser uleselige for ny typed
  klient dersom feltet mangler. Gamle 1.7.2-dokumentdekodere har ikke
  `deny_unknown_fields`, men det erstatter ikke klient- og kompatibilitetstester.
- Kartlegg faktiske konsumenter før release, også bruk av eksisterende
  journalpost-query og testfakes. Ikke anta at ingen deployerte klienter finnes
  fordi ingen ble funnet i undersøkte lokale kildekataloger.
- Deploy serverstøtte og NATS-rettigheter før klientene aktiverer nytt subject.
  Test utrullingsoverlapp dersom delte responstyper endrer svar fra gammel sti.

Schema leveres med ny verifisert release-tag. Skuffens tilsiktede dependency-tag
og root `Cargo.lock` oppdateres samlet; ingen permanent lokal path-patch eller
flytende git-referanse. Commit, release og push gjøres bare etter eksplisitt
oppdrag. Bevar øvrige bibliotekversjoner der oppgradering ikke er nødvendig.

Oppdater som del av implementasjonen:

- `README.md`: nytt subject, eksempler, direkte saksnummeroppslag, dokument-ID,
  hoveddokument først og datetime-format/tidssonesemantikk.
- `.agent/guides/architecture/command/query.md`: metadata versus filinnhold,
  begge saksoppslag og deres inkluderingsoppførsel.
- Relevant beslutning om NATS-kontrakt, SKU-0008, som i dag lister eksakte
  subjects. Les `docs/adr/GOVERNANCE.md`, kjør `adr-fmt --critique SKU-0008`
  før endring og `adr-fmt --lint` etterpå via prosjektets lokale Cargo-kommando.
- Avklar og oppdater overlapp med planen for ferdigbehandling av journalposter:
  dette subjectet og eksisterende responstyper erstatter V2-saksforslaget for
  denne leveransen. Ikke endre andre arbeidskopier uten tillatelse.

Planen er ferdig implementert når testene beviser direkte saksnummeroppslag på
begge subjects og et komplett metadataforløp på det nye, kontrakten er publisert
og kompatibilitetsvurdert, dokumentasjon er oppdatert og nødvendige
driftsrettigheter er verifisert. Ingen påstand om filnedlasting inngår.
