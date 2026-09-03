use anyhow::{Context, Result};
use domain::command::{
    Dekomponeringsinput, DokumentSpesifikasjon, Dokumentkilde as DomeneDokumentkilde,
};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::operasjon::{EntitetType, OperasjonId, dekomponer};
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::typer::{CommandEvent, CommandStatus, StatusErrorCode};
use uuid::Uuid;

use crate::command::materialisering::{
    Dekomponeringsplan, DokumentAttributter, DokumentRad, Dokumentkilde, JournalpostAttributter,
    JournalpostRad, Korrespondanseparter, OperasjonRad, SakAttributter, SakRad, Tilgang,
};
use crate::command::ports::{
    entitet_port::EntitetRepository, operasjon_port::OperasjonRepository,
    status_publisher_port::StatusPublisher,
};
use crate::command::services::ingest_command::{command_type, kontekst};
use crate::command::{
    Command, CommandEnvelope, Dokumentform, JournalpostCommon, OpprettJournalpostCommand, SakKey,
    Tilgjengelighet,
};

/// Utfallet av en dekomponering som ikke lyktes (SKU-0017 R8).
///
/// Bunnen er `Transient`: en ukjent feil retryes til noen legger inn en regel
/// for den. `Permanent` krever positivt treff, og da er kommandoen ferdig —
/// klienten har allerede fått `Feilet`.
#[derive(Debug)]
pub enum DekomponeringsFeil {
    Permanent {
        /// Stabil og greppbar. Går i loggen, ikke til klienten.
        kode: &'static str,
        klientmelding: String,
    },
    Transient(anyhow::Error),
}

impl DekomponeringsFeil {
    fn permanent(kode: &'static str, klientmelding: impl Into<String>) -> Self {
        Self::Permanent {
            kode,
            klientmelding: klientmelding.into(),
        }
    }
}

impl std::fmt::Display for DekomponeringsFeil {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Permanent { kode, .. } => write!(f, "{kode}"),
            Self::Transient(err) => write!(f, "{err:#}"),
        }
    }
}

impl From<anyhow::Error> for DekomponeringsFeil {
    fn from(err: anyhow::Error) -> Self {
        Self::Transient(err)
    }
}

/// Dekomponering fra kommando til operasjoner (SKU-0016 R2).
///
/// Skjer én gang, når den validerte kommandoen leses inn. Operasjonslisten er
/// en ren funksjon av command payload, og alt skrives i én transaksjon sammen
/// med de materialiserte attributtene (R12).
pub struct DekomponerCommandService {
    entitet_repo: Box<dyn EntitetRepository>,
    operasjon_repo: Box<dyn OperasjonRepository>,
    status_publisher: Box<dyn StatusPublisher>,
}

impl DekomponerCommandService {
    pub fn new(
        entitet_repo: Box<dyn EntitetRepository>,
        operasjon_repo: Box<dyn OperasjonRepository>,
        status_publisher: Box<dyn StatusPublisher>,
    ) -> Self {
        Self {
            entitet_repo,
            operasjon_repo,
            status_publisher,
        }
    }

    #[tracing::instrument(
        skip_all,
        name = "command.dekomponer",
        fields(
            command_id = %envelope.command_id,
            correlation_id = tracing::field::Empty,
            command_type = command_type(&envelope.payload).as_code(),
        )
    )]
    pub async fn handle(
        &self,
        envelope: CommandEnvelope<Command>,
    ) -> Result<(), DekomponeringsFeil> {
        if let Some(correlation_id) = envelope.correlation_id {
            tracing::Span::current()
                .record("correlation_id", tracing::field::display(correlation_id));
        }

        let plan = match self.bygg_plan(&envelope).await {
            Ok(plan) => plan,
            Err(DekomponeringsFeil::Permanent {
                kode,
                klientmelding,
            }) => {
                // Beslutning og publisering hører sammen i én kodesti. Uten
                // dette ville klienten sittet igjen med `Validert` og ventet
                // på et utfall som aldri kom.
                self.publiser_feilet(&envelope, &klientmelding).await?;
                return Err(DekomponeringsFeil::Permanent {
                    kode,
                    klientmelding,
                });
            }
            Err(transient) => return Err(transient),
        };

        let resultat = self
            .operasjon_repo
            .lagre_dekomponering(plan)
            .await
            .context("failed to persist decomposition")?;

        // En replay setter inn null rader, så `rows_affected` sier om det var
        // første gang.
        tracing::info!(
            nye_operasjoner = resultat.nye_operasjoner,
            forste_gang = resultat.var_forste_gang(),
            "kommando dekomponert"
        );

        if resultat.var_forste_gang() {
            self.status_publisher
                .publiser_command_status(CommandStatus::new(
                    envelope.command_id,
                    envelope.correlation_id,
                    command_type(&envelope.payload),
                    CommandEvent::Utfores,
                    "Forespørselen utføres.",
                    None,
                    kontekst(&envelope.payload),
                ))
                .await
                .context("failed to publish utfores status")?;
        }

        Ok(())
    }

    /// Terminalt utfall for kommandoen. `Feilet` er monotont (SKU-0016 R8), og
    /// dekomponeringen har ikke skrevet en eneste operasjonsrad, så det kan
    /// ikke motsies senere.
    async fn publiser_feilet(
        &self,
        envelope: &CommandEnvelope<Command>,
        klientmelding: &str,
    ) -> Result<()> {
        self.status_publisher
            .publiser_command_status(CommandStatus::new(
                envelope.command_id,
                envelope.correlation_id,
                command_type(&envelope.payload),
                CommandEvent::Feilet,
                klientmelding,
                // Alle permanente dekomponeringsfeil er feil i det klienten
                // sendte: en ukjent referanse, feil entitetstype, eller for
                // mange dokumenter.
                Some(StatusErrorCode::InvalidRequest),
                kontekst(&envelope.payload),
            ))
            .await
            .context("failed to publish feilet status")
    }

    async fn bygg_plan(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<Dekomponeringsplan, DekomponeringsFeil> {
        let command_id = envelope.command_id;

        match &envelope.payload {
            Command::OpprettSak(command) => {
                let sak_id = self
                    .skuffen_sak_id_for_client_reference(command.client_reference)
                    .await?;
                let input = Dekomponeringsinput::OpprettSak { sak_id };
                let sak = SakRad {
                    sak_id,
                    client_reference: Some(command.client_reference),
                    arkiv_id: None,
                    attributter: Some(SakAttributter {
                        sakstittel: command.sakstittel.clone(),
                        arkivdel: command.arkivdel,
                        ordningsverdi: command.ordningsverdi.get().to_string(),
                        saksbehandler_id: command.saksbehandler_id.clone(),
                        saksbehandler_enhet: command.saksbehandler_enhet.clone(),
                        tilgang: tilgang(&command.tilgjengelighet),
                    }),
                    oensket_saksansvarlig: None,
                };
                Ok(self.plan(command_id, sak, None, Vec::new(), &input))
            }

            Command::AvsluttSak(command) => {
                let sak_id = self.skuffen_sak_id_for_key(&command.sak_key).await?;
                let input = Dekomponeringsinput::AvsluttSak { sak_id };
                let sak = self
                    .eksisterende_sak(sak_id, &command.sak_key, None)
                    .await?;
                Ok(self.plan(command_id, sak, None, Vec::new(), &input))
            }

            Command::SettSaksansvarlig(command) => {
                let sak_id = self.skuffen_sak_id_for_key(&command.sak_key).await?;
                let input = Dekomponeringsinput::SettSaksansvarlig { sak_id };
                let sak = self
                    .eksisterende_sak(
                        sak_id,
                        &command.sak_key,
                        Some((
                            command.saksbehandler_id.clone(),
                            command.saksbehandler_enhet.clone(),
                        )),
                    )
                    .await?;
                Ok(self.plan(command_id, sak, None, Vec::new(), &input))
            }

            Command::OpprettInngaaendeJournalpost(command) => {
                self.journalpost_plan(command_id, command, JournalpostType::Inngaende)
                    .await
            }
            Command::OpprettUtgaaendeJournalpost(command) => {
                self.journalpost_plan(command_id, command, JournalpostType::Utgaaende)
                    .await
            }
            Command::OpprettInterntNotatJournalpost(command) => {
                self.journalpost_plan(command_id, command, JournalpostType::InterntNotat)
                    .await
            }
        }
    }

    async fn journalpost_plan(
        &self,
        command_id: Uuid,
        command: &OpprettJournalpostCommand,
        journalposttype: JournalpostType,
    ) -> Result<Dekomponeringsplan, DekomponeringsFeil> {
        let felles: &JournalpostCommon = command.felles();
        let sak_id = self.skuffen_sak_id_for_key(&felles.sak_key).await?;
        let journalpost_id = self
            .skuffen_journalpost_id_for_client_reference(felles.client_reference)
            .await?;

        let mut dokument_spesifikasjoner = Vec::with_capacity(felles.dokumenter.len());
        let mut dokument_rader = Vec::with_capacity(felles.dokumenter.len());

        // Hoveddokument først i DTO-en. Rekkefølgen gjøres eksplisitt her
        // (D27), i stedet for å overleve som bivirkning av at id-ene genereres
        // i payload-rekkefølge.
        for (indeks, dokument) in felles.dokumenter.iter().enumerate() {
            let rekkefolge = u16::try_from(indeks).map_err(|_| {
                DekomponeringsFeil::permanent(
                    "dekomponering_for_mange_dokumenter",
                    "Journalposten har for mange dokumenter.",
                )
            })?;
            let dokument_id = self
                .skuffen_dokument_id_for_client_reference(dokument.client_reference)
                .await?;

            let (kilde, domenekilde) = match &dokument.form {
                Dokumentform::Bytes {
                    dokument_referanse,
                    filtype,
                } => (
                    Dokumentkilde::Bytes {
                        dokument_referanse: *dokument_referanse,
                        filtype: filtype.clone(),
                    },
                    DomeneDokumentkilde::Bytes,
                ),
                Dokumentform::HtmlTemplate {
                    mal_referanse,
                    felter,
                } => (
                    Dokumentkilde::HtmlTemplate {
                        mal_referanse: *mal_referanse,
                        felter: felter.clone(),
                        rendered_dokument_referanse: None,
                    },
                    DomeneDokumentkilde::HtmlTemplate,
                ),
            };

            dokument_spesifikasjoner.push(DokumentSpesifikasjon {
                dokument_id,
                rekkefolge,
                kilde: domenekilde,
            });
            dokument_rader.push(DokumentRad {
                dokument_id,
                journalpost_id,
                client_reference: dokument.client_reference,
                attributter: DokumentAttributter {
                    tittel: dokument.tittel.clone(),
                    rekkefolge,
                    kilde,
                },
            });
        }

        let med_utsending = command.med_utsending();
        let input = Dekomponeringsinput::OpprettJournalpost {
            sak_id,
            journalpost_id,
            journalposttype,
            med_utsending,
            dokumenter: dokument_spesifikasjoner,
        };

        let sak = self.eksisterende_sak(sak_id, &felles.sak_key, None).await?;
        let journalpost = JournalpostRad {
            journalpost_id,
            client_reference: felles.client_reference,
            attributter: JournalpostAttributter {
                client_reference: felles.client_reference,
                tittel: felles.tittel.clone(),
                dokument_dato: felles.dokument_dato.clone(),
                journalposttype,
                med_utsending,
                saksbehandler_id: felles.saksbehandler.clone(),
                saksbehandler_enhet: felles.saksbehandler_enhet.clone(),
                tilgang: tilgang(&felles.tilgjengelighet),
                korrespondanseparter: korrespondanseparter(command),
                kildesystem: felles.kildesystem.clone(),
            },
        };

        Ok(self.plan(command_id, sak, Some(journalpost), dokument_rader, &input))
    }

    fn plan(
        &self,
        command_id: Uuid,
        sak: SakRad,
        journalpost: Option<JournalpostRad>,
        dokumenter: Vec<DokumentRad>,
        input: &Dekomponeringsinput,
    ) -> Dekomponeringsplan {
        let sak_id = sak.sak_id;
        let operasjoner = dekomponer(input)
            .into_iter()
            .map(|spesifikasjon| OperasjonRad {
                operasjon_id: OperasjonId(Uuid::now_v7()),
                operasjonstype: spesifikasjon.operasjonstype,
                entitet_id: spesifikasjon.entitet_id,
                sak_id,
            })
            .collect();

        Dekomponeringsplan {
            command_id,
            sak,
            journalpost,
            dokumenter,
            operasjoner,
        }
    }

    async fn eksisterende_sak(
        &self,
        sak_id: SkuffenSakId,
        sak_key: &SakKey,
        oensket_saksansvarlig: Option<(String, String)>,
    ) -> Result<SakRad> {
        let (client_reference, arkiv_id) = match sak_key {
            SakKey::ClientReference(client_reference) => (Some(*client_reference), None),
            SakKey::ArkivId(arkiv_id) => (None, Some(arkiv_id.clone())),
        };

        Ok(SakRad {
            sak_id,
            client_reference,
            arkiv_id,
            attributter: None,
            oensket_saksansvarlig,
        })
    }

    async fn skuffen_sak_id_for_key(
        &self,
        sak_key: &SakKey,
    ) -> Result<SkuffenSakId, DekomponeringsFeil> {
        match sak_key {
            SakKey::ClientReference(client_reference) => {
                self.skuffen_sak_id_for_client_reference(*client_reference)
                    .await
            }
            SakKey::ArkivId(arkiv_id) => {
                let skuffen_id = self
                    .entitet_repo
                    .hent_eller_opprett_for_arkiv_id(EntitetType::Sak, arkiv_id)
                    .await
                    .context("failed to resolve sak by arkiv id")?;
                Ok(SkuffenSakId::from(skuffen_id))
            }
        }
    }

    async fn skuffen_sak_id_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<SkuffenSakId, DekomponeringsFeil> {
        Ok(SkuffenSakId::from(
            self.skuffen_id(client_reference, EntitetType::Sak).await?,
        ))
    }

    async fn skuffen_journalpost_id_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<SkuffenJournalpostId, DekomponeringsFeil> {
        Ok(SkuffenJournalpostId::from(
            self.skuffen_id(client_reference, EntitetType::Journalpost)
                .await?,
        ))
    }

    async fn skuffen_dokument_id_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<SkuffenDokumentId, DekomponeringsFeil> {
        Ok(SkuffenDokumentId::from(
            self.skuffen_id(client_reference, EntitetType::Dokument)
                .await?,
        ))
    }

    /// Oversetter klientens referanse til vår `skuffen_id`.
    ///
    /// Feiler hardt hvis entiteten mangler: ingest minter bare id-er for
    /// entiteter kommandoen oppretter, så en manglende rad betyr at validering
    /// slapp gjennom noe. Dekomponering skal ikke reparere det.
    ///
    /// Typesjekken hindrer at en gjenbrukt `client_reference` gir en
    /// journalposts id inn som `sak_id`.
    async fn skuffen_id(
        &self,
        client_reference: Uuid,
        forventet: EntitetType,
    ) -> Result<Uuid, DekomponeringsFeil> {
        let entitet = self
            .entitet_repo
            .hent_for_client_reference(client_reference)
            .await
            .context("failed to look up entitet")?
            .ok_or_else(|| {
                DekomponeringsFeil::permanent(
                    "dekomponering_ukjent_client_reference",
                    "Forespørselen viser til en referanse Skuffen ikke kjenner.",
                )
            })?;

        if entitet.entitet_type != forventet {
            return Err(DekomponeringsFeil::permanent(
                "dekomponering_feil_entitet_type",
                "Referansen peker på en annen entitetstype enn forespørselen krever.",
            ));
        }

        Ok(entitet.skuffen_id)
    }
}

fn tilgang(tilgjengelighet: &Tilgjengelighet) -> Tilgang {
    match tilgjengelighet {
        Tilgjengelighet::Offentlig => Tilgang::Offentlig,
        Tilgjengelighet::Skjermet {
            tilgangskode,
            tilgangshjemmel,
        } => Tilgang::Skjermet {
            tilgangskode: tilgangskode.clone(),
            tilgangshjemmel: tilgangshjemmel.clone(),
        },
    }
}

fn korrespondanseparter(command: &OpprettJournalpostCommand) -> Korrespondanseparter {
    match command {
        OpprettJournalpostCommand::Inngaende { avsender, .. } => {
            Korrespondanseparter::Avsender(avsender.clone())
        }
        OpprettJournalpostCommand::Utgaaende { mottakere, .. } => {
            Korrespondanseparter::Mottakere(mottakere.clone())
        }
        OpprettJournalpostCommand::UtgaaendeMedUtsending { mottakere, .. } => {
            Korrespondanseparter::Utsendingsmottakere(mottakere.clone())
        }
        OpprettJournalpostCommand::InterntNotat { .. } => Korrespondanseparter::Ingen,
    }
}
