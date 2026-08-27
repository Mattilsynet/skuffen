use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use domain::eksekvering::id::SkuffenDokumentId;
use domain::eksekvering::operasjon::{
    Beslutning, EntitetId, Operasjon, Operasjonstype, muterer_arkivet, vurder, vurder_avslutt_sak,
};
use domain::eksekvering::tilstand::{JournalpostTilstand, SakMedBarn};
use domain::eksekvering::typer::{
    CommandEvent, CommandStatus, EksekveringFeil, Operasjonshendelse, Operasjonstatus,
    StatusErrorCode,
};

use crate::command::ports::operasjon_port::CommandMetadata;

use crate::command::materialisering::Dokumentkilde;
use crate::command::ports::{
    eksekvering_port::{ArkivGateway, Journalstatus, ObservertJournalstatus},
    fakta_port::FaktaRepository,
    operasjon_port::{CommandOutcome, Faktaoppdatering, OperasjonRepository},
    status_publisher_port::StatusPublisher,
};

enum Utfall {
    Ferdig(Faktaoppdatering),
    /// Skriv observerte fakta og prøv igjen senere.
    PollIgjen {
        oppdatering: Faktaoppdatering,
        om: Duration,
    },
}

/// Skriveoperasjoner går i to faser (SKU-0016 R4): `klar → sendt` commites før
/// arkivkallet, og `sendt → ok` med arkivsvar og faktaoppdatering i én
/// transaksjon etterpå. Der ligger at-most-once-grensen — en DB-feil etter et
/// vellykket arkivskriv gir `krever_avklaring`, ikke et duplikat.
pub struct EksekverOperasjonService {
    operasjon: Box<dyn OperasjonRepository>,
    fakta: Box<dyn FaktaRepository>,
    gateway: Box<dyn ArkivGateway>,
    render: Box<dyn RenderOperasjon>,
    publisher: Box<dyn StatusPublisher>,
    executor_id: String,
    poll_intervall: Duration,
}

/// Egen seam fordi rendring går mot object store, ikke mot arkivet.
#[async_trait::async_trait]
pub trait RenderOperasjon: Send + Sync {
    async fn render(
        &self,
        dokument_id: SkuffenDokumentId,
        mal_referanse: uuid::Uuid,
        felter: &[domain::eksekvering::html_template::TemplateFelt],
        saksnummer: Option<&str>,
    ) -> Result<uuid::Uuid, EksekveringFeil>;
}

impl EksekverOperasjonService {
    pub fn new(
        operasjon: Box<dyn OperasjonRepository>,
        fakta: Box<dyn FaktaRepository>,
        gateway: Box<dyn ArkivGateway>,
        render: Box<dyn RenderOperasjon>,
        publisher: Box<dyn StatusPublisher>,
        executor_id: impl Into<String>,
        poll_intervall: Duration,
    ) -> Self {
        Self {
            operasjon,
            fakta,
            gateway,
            render,
            publisher,
            executor_id: executor_id.into(),
            poll_intervall,
        }
    }

    /// Kjører neste kjørbare operasjon. `Ok(false)` betyr at køen er tom.
    pub async fn run_next(&self) -> Result<bool> {
        let Some(op) = self.operasjon.hent_neste_kjorbare().await? else {
            return Ok(false);
        };
        self.execute(op).await?;
        Ok(true)
    }

    pub async fn execute(&self, op: Operasjon) -> Result<()> {
        let facts = self
            .fakta
            .hent_sak_med_barn(op.sak_id)
            .await?
            .ok_or_else(|| anyhow!("sak facts missing for operasjon"))?;

        match self.beslutt(&op, &facts).await? {
            Beslutning::Blokkert(grunn) => {
                // Blokkeringsårsak er spørrbar tilstand (D33), så vi
                // publiserer ikke ved `blokkert ↔ klar`-flakking.
                self.operasjon
                    .marker_blokkert(op.operasjon_id, None, &grunn.safe_detail())
                    .await?;
            }
            Beslutning::AlleredeUtfort => {
                self.operasjon
                    .fullfor_ok(op.operasjon_id, 0, Faktaoppdatering::Ingen)
                    .await?;
                self.publiser(&op, Operasjonshendelse::Ok, 0, "Allerede utført.", None)
                    .await?;
            }
            Beslutning::Ugyldig(brudd) => {
                self.operasjon
                    .marker_feilet(op.operasjon_id, 0, &brudd.safe_detail())
                    .await?;
                self.publiser(
                    &op,
                    Operasjonshendelse::Feilet,
                    0,
                    "Operasjonen kan ikke utføres.",
                    Some(StatusErrorCode::ProcessingFailed),
                )
                .await?;
            }
            Beslutning::Utfor => self.utfor(&op, &facts).await?,
        }

        Ok(())
    }

    async fn beslutt(&self, op: &Operasjon, facts: &SakMedBarn) -> Result<Beslutning> {
        if op.operasjonstype == Operasjonstype::AvsluttSak {
            // Eneste unntak fra facts-only-regelen (D4): den trenger å vite at
            // alle andre operasjoner på saken er terminalt ok.
            let sosken = self.operasjon.hent_sammendrag_for_sak(op.sak_id).await?;
            Ok(vurder_avslutt_sak(op, facts, &sosken))
        } else {
            Ok(vurder(op, facts))
        }
    }

    async fn utfor(&self, op: &Operasjon, facts: &SakMedBarn) -> Result<()> {
        let attempt_no = self
            .operasjon
            .marker_kjorer(op.operasjon_id, &self.executor_id)
            .await?;

        // At-most-once-grensen. Idempotente operasjoner hopper over den og kan
        // retryes fritt i stedet for å havne i `krever_avklaring`.
        if muterer_arkivet(op.operasjonstype) {
            self.operasjon
                .marker_sendt(op.operasjon_id, attempt_no)
                .await?;
        }

        match self.arkivkall(op, facts).await {
            Ok(Utfall::Ferdig(oppdatering)) => {
                self.operasjon
                    .fullfor_ok(op.operasjon_id, attempt_no, oppdatering)
                    .await
                    .context("failed to commit successful operation")?;
                self.publiser(op, Operasjonshendelse::Ok, attempt_no, "Utført.", None)
                    .await?;
            }
            Ok(Utfall::PollIgjen { oppdatering, om }) => {
                let neste = chrono::Utc::now()
                    + chrono::Duration::from_std(om).unwrap_or(chrono::Duration::hours(1));
                self.operasjon
                    .fullfor_poll(op.operasjon_id, attempt_no, oppdatering, neste)
                    .await?;
            }
            Err(feil) if feil.er_recoverable() => {
                // Retryes for alltid, med backoff opp til én gang per døgn
                // (SKU-0016 R6). Ingen maks antall forsøk.
                let neste = crate::command::services::eksekvering_backoff::neste_backoff(
                    attempt_no.max(1) as u32 - 1,
                );
                self.operasjon
                    .marker_retry(op.operasjon_id, attempt_no, &feil.siste_detalj(), neste)
                    .await?;
                self.publiser(
                    op,
                    Operasjonshendelse::ForsokFeilet,
                    attempt_no,
                    &feil.melding,
                    Some(feil.error_code),
                )
                .await?;
            }
            Err(feil) => {
                self.operasjon
                    .marker_feilet(op.operasjon_id, attempt_no, &feil.siste_detalj())
                    .await?;
                self.publiser(
                    op,
                    Operasjonshendelse::Feilet,
                    attempt_no,
                    &feil.melding,
                    Some(feil.error_code),
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn arkivkall(
        &self,
        op: &Operasjon,
        facts: &SakMedBarn,
    ) -> Result<Utfall, EksekveringFeil> {
        match op.operasjonstype {
            Operasjonstype::OpprettSak => self.opprett_sak(op).await,
            Operasjonstype::SettSaksansvarlig => self.sett_saksansvarlig(facts).await,
            Operasjonstype::AvsluttSak => self.avslutt_sak(facts).await,
            Operasjonstype::RenderDokument => self.render_dokument(op, facts).await,
            Operasjonstype::OpprettJournalpost => self.opprett_journalpost(op, facts).await,
            Operasjonstype::LeggTilVedlegg => self.legg_til_vedlegg(op, facts).await,
            Operasjonstype::Journalfor => {
                self.statusovergang(
                    op,
                    facts,
                    Journalstatus::Journalfoert,
                    JournalpostTilstand::Journalfoert,
                )
                .await
            }
            Operasjonstype::SettEkspedert => {
                self.statusovergang(
                    op,
                    facts,
                    Journalstatus::Ekspedert,
                    JournalpostTilstand::Ekspedert,
                )
                .await
            }
            Operasjonstype::KlargjorForEkspedering => {
                self.statusovergang(
                    op,
                    facts,
                    Journalstatus::KlarForEkspedering,
                    JournalpostTilstand::KlarForEkspedering,
                )
                .await
            }
            Operasjonstype::AvventJournalfort => self.avvent_journalfort(op, facts).await,
            Operasjonstype::Avskriv => self.avskriv(op, facts).await,
        }
    }

    // --- sak ---

    async fn opprett_sak(&self, op: &Operasjon) -> Result<Utfall, EksekveringFeil> {
        let attributter = self
            .fakta
            .hent_sak_attributter(op.sak_id)
            .await
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_sak_attributter_utilgjengelig")
                    .med_intern_detalj(err.to_string())
            })?
            .ok_or_else(|| EksekveringFeil::intern("intern_sak_attributter_mangler"))?;

        let resultat = self.gateway.opprett_sak(&attributter).await?;

        Ok(Utfall::Ferdig(Faktaoppdatering::SakOpprettet {
            arkiv_id: resultat.saksnummer,
        }))
    }

    async fn sett_saksansvarlig(&self, facts: &SakMedBarn) -> Result<Utfall, EksekveringFeil> {
        let saksnummer = krev_arkiv_id(facts.arkiv_id.as_deref(), "intern_sak_arkiv_id_mangler")?;
        let oensket = facts
            .oensket_saksansvarlig
            .as_ref()
            .ok_or_else(|| EksekveringFeil::intern("intern_oensket_saksansvarlig_mangler"))?;

        self.gateway
            .sett_saksansvarlig(saksnummer, &oensket.saksbehandler_id, &oensket.enhet)
            .await?;

        Ok(Utfall::Ferdig(Faktaoppdatering::SaksansvarligSatt {
            saksbehandler_id: oensket.saksbehandler_id.clone(),
            saksbehandler_enhet: oensket.enhet.clone(),
        }))
    }

    async fn avslutt_sak(&self, facts: &SakMedBarn) -> Result<Utfall, EksekveringFeil> {
        let saksnummer = krev_arkiv_id(facts.arkiv_id.as_deref(), "intern_sak_arkiv_id_mangler")?;
        self.gateway.avslutt_sak(saksnummer).await?;
        Ok(Utfall::Ferdig(Faktaoppdatering::SakAvsluttet))
    }

    // --- dokument ---

    async fn render_dokument(
        &self,
        op: &Operasjon,
        facts: &SakMedBarn,
    ) -> Result<Utfall, EksekveringFeil> {
        let dokument_id = krev_dokument(op)?;
        let attributter = self
            .fakta
            .hent_dokument_attributter(dokument_id)
            .await
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_dokument_attributter_utilgjengelig")
                    .med_intern_detalj(err.to_string())
            })?
            .ok_or_else(|| EksekveringFeil::intern("intern_dokument_attributter_mangler"))?;

        let Dokumentkilde::HtmlTemplate {
            mal_referanse,
            felter,
            rendered_dokument_referanse,
        } = &attributter.kilde
        else {
            return Err(EksekveringFeil::intern("intern_dokument_er_ikke_mal"));
        };

        // Avbrutt forsøk kan ha lagret PDF-en allerede (SKU-0005 R10).
        if let Some(referanse) = rendered_dokument_referanse {
            return Ok(Utfall::Ferdig(Faktaoppdatering::DokumentRendret {
                dokument_id,
                rendered_dokument_referanse: *referanse,
            }));
        }

        let referanse = self
            .render
            .render(
                dokument_id,
                *mal_referanse,
                felter,
                facts.arkiv_id.as_deref(),
            )
            .await?;

        Ok(Utfall::Ferdig(Faktaoppdatering::DokumentRendret {
            dokument_id,
            rendered_dokument_referanse: referanse,
        }))
    }

    async fn legg_til_vedlegg(
        &self,
        op: &Operasjon,
        facts: &SakMedBarn,
    ) -> Result<Utfall, EksekveringFeil> {
        let dokument_id = krev_dokument(op)?;
        let (journalpost, _) = facts
            .dokument(dokument_id)
            .ok_or_else(|| EksekveringFeil::intern("intern_dokument_fakta_mangler"))?;
        let journalpost_arkiv_id = krev_journalpost_arkiv_id(journalpost.arkiv_id.as_deref())?;

        let attributter = self
            .fakta
            .hent_dokument_attributter(dokument_id)
            .await
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_dokument_attributter_utilgjengelig")
                    .med_intern_detalj(err.to_string())
            })?
            .ok_or_else(|| EksekveringFeil::intern("intern_dokument_attributter_mangler"))?;

        let vedlegg_id = self
            .gateway
            .legg_til_vedlegg(journalpost_arkiv_id, &attributter)
            .await?;

        Ok(Utfall::Ferdig(Faktaoppdatering::VedleggArkivert {
            dokument_id,
            arkiv_id: vedlegg_id.map(|id| id.to_string()),
        }))
    }

    // --- journalpost ---

    async fn opprett_journalpost(
        &self,
        op: &Operasjon,
        facts: &SakMedBarn,
    ) -> Result<Utfall, EksekveringFeil> {
        let journalpost_id = krev_journalpost(op)?;
        let saksnummer = krev_arkiv_id(facts.arkiv_id.as_deref(), "intern_sak_arkiv_id_mangler")?;

        let attributter = self
            .fakta
            .hent_journalpost_attributter(journalpost_id)
            .await
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_journalpost_attributter_utilgjengelig")
                    .med_intern_detalj(err.to_string())
            })?
            .ok_or_else(|| EksekveringFeil::intern("intern_journalpost_attributter_mangler"))?;

        let dokumenter = self
            .fakta
            .hent_dokumenter_for_journalpost(journalpost_id)
            .await
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_dokumenter_utilgjengelig")
                    .med_intern_detalj(err.to_string())
            })?;
        let (hoveddokument_id, hoveddokument) = dokumenter
            .into_iter()
            .find(|(_, dok)| dok.er_hoveddokument())
            .ok_or_else(|| EksekveringFeil::intern("intern_hoveddokument_mangler"))?;

        let resultat = self
            .gateway
            .opprett_journalpost(saksnummer, &attributter, &hoveddokument)
            .await?;

        Ok(Utfall::Ferdig(Faktaoppdatering::JournalpostOpprettet {
            journalpost_id,
            arkiv_id: resultat.journalpost_id.to_string(),
            hoveddokument_id,
        }))
    }

    async fn statusovergang(
        &self,
        op: &Operasjon,
        facts: &SakMedBarn,
        status: Journalstatus,
        tilstand: JournalpostTilstand,
    ) -> Result<Utfall, EksekveringFeil> {
        let journalpost_id = krev_journalpost(op)?;
        let arkiv_id = self.journalpost_arkiv_id(facts, op)?;

        self.gateway
            .sett_journalpost_status(arkiv_id, status)
            .await?;

        Ok(Utfall::Ferdig(Faktaoppdatering::JournalpostStatus {
            journalpost_id,
            tilstand,
        }))
    }

    async fn avskriv(&self, op: &Operasjon, facts: &SakMedBarn) -> Result<Utfall, EksekveringFeil> {
        let journalpost_id = krev_journalpost(op)?;
        let arkiv_id = self.journalpost_arkiv_id(facts, op)?;
        let attributter = self
            .fakta
            .hent_journalpost_attributter(journalpost_id)
            .await
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_journalpost_attributter_utilgjengelig")
                    .med_intern_detalj(err.to_string())
            })?
            .ok_or_else(|| EksekveringFeil::intern("intern_journalpost_attributter_mangler"))?;

        self.gateway
            .avskriv_journalpost(arkiv_id, attributter.kildesystem.as_deref(), None)
            .await?;

        Ok(Utfall::Ferdig(Faktaoppdatering::JournalpostStatus {
            journalpost_id,
            tilstand: JournalpostTilstand::Avskrevet,
        }))
    }

    /// Poller til RPA har satt `J`. Observert `E` skrives som fakta underveis
    /// (D20), slik at faktabildet er sant selv mens vi venter.
    async fn avvent_journalfort(
        &self,
        op: &Operasjon,
        facts: &SakMedBarn,
    ) -> Result<Utfall, EksekveringFeil> {
        let journalpost_id = krev_journalpost(op)?;
        let arkiv_id = self.journalpost_arkiv_id(facts, op)?;

        let observert = self.gateway.hent_journalstatus(arkiv_id).await?;

        match observert {
            ObservertJournalstatus::Journalfoert => {
                Ok(Utfall::Ferdig(Faktaoppdatering::JournalpostStatus {
                    journalpost_id,
                    tilstand: JournalpostTilstand::Journalfoert,
                }))
            }
            ObservertJournalstatus::Ekspedert => Ok(Utfall::PollIgjen {
                oppdatering: Faktaoppdatering::JournalpostStatus {
                    journalpost_id,
                    tilstand: JournalpostTilstand::Ekspedert,
                },
                om: self.poll_intervall,
            }),
            _ => Ok(Utfall::PollIgjen {
                oppdatering: Faktaoppdatering::Ingen,
                om: self.poll_intervall,
            }),
        }
    }

    fn journalpost_arkiv_id(
        &self,
        facts: &SakMedBarn,
        op: &Operasjon,
    ) -> Result<i32, EksekveringFeil> {
        let journalpost_id = krev_journalpost(op)?;
        let journalpost = facts
            .journalpost(journalpost_id)
            .ok_or_else(|| EksekveringFeil::intern("intern_journalpost_fakta_mangler"))?;
        krev_journalpost_arkiv_id(journalpost.arkiv_id.as_deref())
    }

    async fn publiser(
        &self,
        op: &Operasjon,
        hendelse: Operasjonshendelse,
        attempt_no: i32,
        melding: &str,
        error_code: Option<StatusErrorCode>,
    ) -> Result<()> {
        let command = self
            .operasjon
            .hent_command_metadata(op.operasjon_id)
            .await?;

        self.publisher
            .publiser_operasjonstatus(Operasjonstatus::new(
                command.command_id,
                command.correlation_id,
                op.operasjon_id,
                op.operasjonstype,
                hendelse,
                attempt_no,
                melding,
                error_code,
            ))
            .await?;

        if hendelse.er_terminal() {
            self.publiser_command_outcome(&command).await?;
        }

        Ok(())
    }

    /// Terminal feil publiseres umiddelbart, ikke ved quiescence (SKU-0016 R8).
    /// Foldet er monotont — når én operasjon har feilet terminalt kan
    /// resultatet aldri gå tilbake til ok — så eventet er sant i det øyeblikket
    /// det sendes og kan aldri trekkes tilbake.
    async fn publiser_command_outcome(&self, command: &CommandMetadata) -> Result<()> {
        let utfall = self
            .operasjon
            .hent_command_outcome(command.command_id)
            .await?;

        let (hendelse, melding, error_code) = match utfall {
            CommandOutcome::Uavklart => return Ok(()),
            CommandOutcome::Fullfort => {
                (CommandEvent::Fullfort, "Forespørselen er fullført.", None)
            }
            CommandOutcome::Feilet => (
                CommandEvent::Feilet,
                "Forespørselen kunne ikke fullføres.",
                Some(StatusErrorCode::ProcessingFailed),
            ),
        };

        self.publisher
            .publiser_command_status(CommandStatus::new(
                command.command_id,
                command.correlation_id,
                command.command_type,
                hendelse,
                melding,
                error_code,
                command.kontekst.clone(),
            ))
            .await
    }
}

fn krev_journalpost(
    op: &Operasjon,
) -> Result<domain::eksekvering::id::SkuffenJournalpostId, EksekveringFeil> {
    match op.entitet_id {
        EntitetId::Journalpost(id) => Ok(id),
        _ => Err(EksekveringFeil::intern(
            "intern_forventet_journalpost_entitet",
        )),
    }
}

fn krev_dokument(op: &Operasjon) -> Result<SkuffenDokumentId, EksekveringFeil> {
    match op.entitet_id {
        EntitetId::Dokument(id) => Ok(id),
        _ => Err(EksekveringFeil::intern("intern_forventet_dokument_entitet")),
    }
}

fn krev_arkiv_id<'a>(
    arkiv_id: Option<&'a str>,
    kode: &'static str,
) -> Result<&'a str, EksekveringFeil> {
    arkiv_id.ok_or_else(|| EksekveringFeil::intern(kode))
}

fn krev_journalpost_arkiv_id(arkiv_id: Option<&str>) -> Result<i32, EksekveringFeil> {
    krev_arkiv_id(arkiv_id, "intern_journalpost_arkiv_id_mangler")?
        .parse::<i32>()
        .map_err(|_| EksekveringFeil::intern("intern_journalpost_arkiv_id_ikke_numerisk"))
}
