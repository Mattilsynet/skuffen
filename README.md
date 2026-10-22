# Skuffen

Grensesnittet er meldingsbasert over **NATS**, og arkivering skjer **asynkront**.
Skuffen tar hånd om validering, avhengigheter mellom arkivoperasjoner og nye forsøk
ved midlertidige feil. Repoet er offentlig, men tjenesten er laget for intern bruk
i Mattilsynet; integrasjon krever tilgang til NATS i det aktuelle miljøet.

## Slik fungerer en arkivering

1. **Last opp filene først.** Klienten oppretter en UUID for hver opplasting og
   laster opp filen over NATS med chunked upload-protokollen under
   `arkiv.arkiver.media`. Fullfør opplastingen før du bruker UUID-en som
   `dokument_referanse` i en kommando.
2. **Send kommandoene i riktig rekkefølge.** for eksempel opprett sak → opprett
   journalpost → avslutt sak. Hver kommando har sin egen `command_id`.
3. **Ta vare på `command_id`.** `Ok` betyr at hele listen er mottatt for videre
   validering og utførelse — **ikke at arkiveringen er fullført**.
4. **Følg status i JetStream.** Skuffen publiserer fremdrift og utfall per
   `command_id`. Klienten må følge disse hendelsene for å vite hvordan det gikk.

Du trenger ikke vente på et saksnummer fra arkivet før du sender neste kommando.
Gi saken en `client_reference`, og bruk den når journalposten peker på saken.
Da kan begge sendes i samme liste, også når arkivet er utilgjengelig.

### ID-er klienten holder styr på

| Felt | Bruk |
| --- | --- |
| `command_id` | UUID for én kommando; brukes til idempotens og til å følge status. Behold ID-en for å følge status. |
| `client_reference` | Klientens UUID for en sak, journalpost eller et dokument, uavhengig av arkivets ID. |
| `dokument_referanse` | UUID-en til en ferdig opplastet fil; er ikke det samme som dokumentets `client_reference`. |
| `correlation_id` | Valgfri UUID som knytter flere kommandoer sammen for sporbarhet. |

## Kontrakter og filopplasting

De delte kontraktene ligger i `landdyrtilsyn-libs`:

- **[DTO-er i lib-schemas](https://github.com/Mattilsynet/landdyrtilsyn-libs/tree/master/lib-schemas/src/skuffen):**
  Rust-typene som definerer JSON-formatet for kommandoer, queries, svar og
  statushendelser. Bruk disse som referanse også når klienten skrives i et annet språk.
- **[Chunked upload i lib-nats](https://github.com/Mattilsynet/landdyrtilsyn-libs/tree/master/lib-nats/src/chunked_upload):**
  Protokoll og Rust-klient for filopplasting. `begin` oppretter en session,
  filen sendes i chunks til den tildelte mottakeren, og `commit` fullfører
  opplastingen. [Protokollbeskrivelsen](https://github.com/Mattilsynet/landdyrtilsyn-libs/blob/master/lib-nats/README.md)
  dokumenterer meldingsformat, svar og retry for klienter i alle språk.

### Eksempel: opprett sak → opprett internt notat → avslutt sak

Send de tre kommandoene samlet til `arkiv.arkiver`. Notatet og avslutningen
refererer til sakens `client_reference`, ikke til kommando-ID-en.

Før du sender:

- Last opp en PDF med en egen UUID som `upload_id`
  via opplastingsprotokollen, og vent på vellykket lagring. Denne ID-en brukes
  som `dokument_referanse` under. PDF-en er notatets hoveddokument.
- Kontroller at saksbehandler, enhet og arkivmetadata er gyldige i testmiljøet.
- Erstatt `<UUID>` med egne UUID-er: én unik per kommando og per
  `client_reference`, og en felles `correlation_id` for sekvensen. Begge
  `sak_key.value` skal være sakens `client_reference`; `dokument_referanse`
  skal være filens `upload_id`. Ikke send ekte personopplysninger i testdata.

```bash
nats request arkiv.arkiver '[
  {
    "command_id": "<UUID>",
    "correlation_id": "<UUID>",
    "payload": {
      "OpprettSak": {
        "client_reference": "<UUID>",
        "sakstittel": "Eksempelsak",
        "arkivdel": "Tilsynsdivisjonene",
        "saksbehandler_id": "17804",
        "saksbehandler_enhet": "M32000",
        "ordningsverdi": "123",
        "tilgjengelighet": {
          "Skjermet": {
            "tilgangskode": "UO",
            "tilgangshjemmel": "Offl. § 24 andre ledd"
          }
        }
      }
    }
  },
  {
    "command_id": "<UUID>",
    "correlation_id": "<UUID>",
    "payload": {
      "OpprettInterntNotatJournalpost": {
        "client_reference": "<UUID>",
        "tittel": "Internt fagnotat",
        "dokument_dato": "2026-01-01",
        "saksbehandler": "17804",
        "saksbehandler_enhet": "M32000",
        "tilgjengelighet": {
          "Skjermet": {
            "tilgangskode": "UO",
            "tilgangshjemmel": "Offl. § 14 første ledd"
          }
        },
        "dokumenter": [
          {
            "client_reference": "<UUID>",
            "tittel": "Fagnotat",
            "form": {
              "Bytes": {
                "dokument_referanse": "<UUID>",
                "filtype": "PDF"
              }
            }
          }
        ],
        "sak_key": {
          "type": "clientReference",
          "value": "<UUID>"
        }
      }
    }
  },
  {
    "command_id": "<UUID>",
    "correlation_id": "<UUID>",
    "payload": {
      "AvsluttSak": {
        "sak_key": {
          "type": "clientReference",
          "value": "<UUID>"
        }
      }
    }
  }
]'
```

En vellykket mottakskvittering inneholder de tre innsendte kommando-ID-ene i samme
rekkefølge:

```json
{
  "Ok": {
    "command_ids": [
      "<UUID>",
      "<UUID>",
      "<UUID>"
    ]
  }
}
```

Feilsvar har formatet `{"Error":{"message":"..."}}`. Ved timeout eller teknisk
feil må du ikke anta at ingen kommandoer er mottatt; behold de opprinnelige
kommando-ID-ene ved nytt forsøk.

## Følg status og håndter feil

Status publiseres i JetStream-strømmen `arkiv_status`:

| Subject / filter | Innhold |
| --- | --- |
| `arkiv.status.<command_id>.command` | Kommandoens fremdrift og utfall, som `SkuffenCommandStatusV1`. |
| `arkiv.status.<command_id>.operasjon.<operasjon_id>` | Status for én operasjon (deloppgave), som `SkuffenOperasjonStatusV1`. |
| `arkiv.status.<command_id>.>` | Alle hendelser for kommandoen. |


- `fullfort` betyr at kommandoen er fullført; `avvist` og `feilet` melder feil.
- Midlertidige feil forsøkes på nytt av Skuffen. Klienten trenger ikke sende nye
  kommandoer for å holde arbeidet i gang.
- `krever_avklaring` betyr at et arkivkall har ukjent utfall og trenger manuell
  avklaring.
- `terminal: true` markerer et avgjort utfall, men flere operasjonshendelser kan
  komme etterpå. Allerede utførte arkivoperasjoner rulles ikke tilbake ved feil.
- Operatørstyrt retry kan gjenåpne avviste eller feilede kommandoer på samme
  `command_id`. Klienten må derfor kunne ta imot senere statusoppdateringer.

## Lese fra arkivet

Lesing er synkron NATS request-reply:

- `arkiv.request.sak.hent` henter en sak uten journalposter.
- `arkiv.request.sak.med_journalposter` henter en sak med journalposter og
  dokumentmetadata, men uten filinnhold.

Begge tar en saksnøkkel med `clientReference` eller `arkivId` (saksnummer).
Svarene bruker `NatsResponse<T>` med `status` og `payload`, ikke samme format som
arkiveringskvitteringen. Se [query-guiden](.agent/guides/architecture/command/query.md)
for detaljer. Enkeltoppslag av journalpost og bruker/MT-enheter er foreløpig ikke
implementert mot det ekte arkivet.

## Utvikle og bidra

Skuffen er skrevet i Rust. Bruk stable-toolchain og Docker for å kjøre testene
(også enkelte repository-tester starter PostgreSQL i Docker):

```bash
cargo test --workspace --exclude skuffen-integration-tests
```

Integrasjonstestene starter NATS og PostgreSQL selv og bruker fake Sikri-adaptere:

```bash
cargo test -p skuffen-integration-tests
```

Oppsett, manuell testing og kvalitetssjekker ligger i
[utviklingsguiden](.agent/guides/commands.md).
Spørsmål, konstruktive tilbakemeldinger, issues og pull requests er velkomne.
