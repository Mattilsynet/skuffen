use anyhow::{Context, Result};
use domain::eksekvering::operasjon::EntitetType;
use domain::eksekvering::typer::{CommandEvent, CommandStatus, CommandTypeCode, Statuskontekst};
use uuid::Uuid;

use crate::command::ports::{
    command_dispatcher_port::CommandDispatcher,
    command_port::CommandRepository,
    entitet_port::{EntitetRepository, NyEntitet},
    status_publisher_port::StatusPublisher,
};
use crate::command::{Command, CommandEnvelope, SakKey};

/// Idempotency-nøkkelen er `command.dispatchet_at`, ikke radens eksistens
/// (SKU-0016 R11). Rekkefølgen er: skriv mottaksraden, mint id-ene, dispatch,
/// og først da sett milepælen. Feiler dispatch, står milepælen som `NULL`, og
/// en klient-retry sender kommandoen på nytt.
pub struct IngestCommandService {
    command: Box<dyn CommandRepository>,
    entitet: Box<dyn EntitetRepository>,
    dispatcher: Box<dyn CommandDispatcher>,
    status_publisher: Box<dyn StatusPublisher>,
}

impl IngestCommandService {
    pub fn new(
        command: Box<dyn CommandRepository>,
        entitet: Box<dyn EntitetRepository>,
        dispatcher: Box<dyn CommandDispatcher>,
        status_publisher: Box<dyn StatusPublisher>,
    ) -> Self {
        Self {
            command,
            entitet,
            dispatcher,
            status_publisher,
        }
    }

    /// Returnerer alle command-id-er i innsendt rekkefølge, inkludert
    /// idempotent aksepterte (SKU-0008 R3). Feiler én, feiler hele batchen.
    pub async fn handle(&self, commands: Vec<CommandEnvelope<Command>>) -> Result<Vec<Uuid>> {
        let mut command_ids = Vec::new();

        for envelope in commands {
            let command_id = envelope.command_id;
            self.process_command(envelope).await?;
            command_ids.push(command_id);
        }

        Ok(command_ids)
    }

    #[tracing::instrument(
        skip_all,
        name = "command.ingest",
        fields(
            command_id = %envelope.command_id,
            correlation_id = tracing::field::Empty,
            command_type = command_type(&envelope.payload).as_code(),
        )
    )]
    async fn process_command(&self, envelope: CommandEnvelope<Command>) -> Result<()> {
        let command_id = envelope.command_id;
        if let Some(correlation_id) = envelope.correlation_id {
            tracing::Span::current()
                .record("correlation_id", tracing::field::display(correlation_id));
        }

        let mottak = self
            .command
            .registrer_mottatt(&envelope)
            .await
            .context("failed to record command receipt")?;

        if !mottak.maa_dispatches() {
            tracing::info!("kommando allerede dispatchet, hopper over");
            return Ok(());
        }

        self.registrer_entiteter(&envelope).await?;

        self.dispatcher
            .dispatch(&envelope)
            .await
            .context("failed to dispatch command")?;

        // Milepælen settes først når dispatch faktisk lyktes.
        self.command
            .marker_dispatchet(command_id)
            .await
            .context("failed to mark command dispatched")?;

        self.status_publisher
            .publiser_command_status(CommandStatus::new(
                command_id,
                envelope.correlation_id,
                command_type(&envelope.payload),
                CommandEvent::Mottatt,
                "Forespørselen er mottatt.",
                None,
                kontekst(&envelope.payload),
            ))
            .await
            .context("failed to publish mottatt status")?;

        tracing::info!("kommando mottatt og dispatchet");
        Ok(())
    }

    /// Minter `skuffen_id` for entitetene kommandoen oppretter.
    ///
    /// Skjer før validering, som er grunnen til at `entitet` er en egen tabell:
    /// id-ene kan deles ut og kommandoen så bli avvist (SKU-0016 R11).
    async fn registrer_entiteter(&self, envelope: &CommandEnvelope<Command>) -> Result<()> {
        match &envelope.payload {
            Command::OpprettSak(command) => {
                self.registrer_entitet(EntitetType::Sak, Some(command.client_reference), None)
                    .await?;
            }
            // AvsluttSak og SettSaksansvarlig oppretter ingenting — de virker
            // på en sak som må finnes fra før. Mintet vi en id for en ukjent
            // client_reference her, ville validering ikke lenger kunne se
            // forskjell på en ukjent sak og en vi selv har opprettet.
            Command::AvsluttSak(command) => {
                self.knytt_arkiv_id(&command.sak_key).await?;
            }
            Command::SettSaksansvarlig(command) => {
                self.knytt_arkiv_id(&command.sak_key).await?;
            }
            Command::OpprettInngaaendeJournalpost(command)
            | Command::OpprettUtgaaendeJournalpost(command)
            | Command::OpprettInterntNotatJournalpost(command) => {
                let felles = command.felles();
                // Saken må finnes fra før, enten fra en OpprettSak tidligere i
                // batchen eller via arkiv-id.
                self.knytt_arkiv_id(&felles.sak_key).await?;
                self.registrer_entitet(
                    EntitetType::Journalpost,
                    Some(felles.client_reference),
                    None,
                )
                .await?;
                for dokument in &felles.dokumenter {
                    self.registrer_entitet(
                        EntitetType::Dokument,
                        Some(dokument.client_reference),
                        None,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    /// En arkiv-id er et eksternt faktum vi bringer inn i vår identitetsmodell;
    /// den kan derfor opprettes. En client_reference kan ikke — den må komme
    /// fra en kommando som faktisk oppretter saken.
    async fn knytt_arkiv_id(&self, sak_key: &SakKey) -> Result<()> {
        if let SakKey::ArkivId(arkiv_id) = sak_key {
            self.entitet
                .hent_eller_opprett_for_arkiv_id(EntitetType::Sak, arkiv_id)
                .await
                .context("failed to resolve sak by arkiv id")?;
        }
        Ok(())
    }

    async fn registrer_entitet(
        &self,
        entitet_type: EntitetType,
        client_reference: Option<Uuid>,
        arkiv_id: Option<String>,
    ) -> Result<()> {
        self.entitet
            .registrer(NyEntitet {
                skuffen_id: Uuid::now_v7(),
                entitet_type,
                client_reference,
                arkiv_id,
            })
            .await
            .context("failed to register entitet")?;
        Ok(())
    }
}

pub fn command_type(command: &Command) -> CommandTypeCode {
    match command {
        Command::OpprettSak(_) => CommandTypeCode::OpprettSak,
        Command::OpprettInngaaendeJournalpost(_) => CommandTypeCode::OpprettInngaaendeJournalpost,
        Command::OpprettUtgaaendeJournalpost(_) => CommandTypeCode::OpprettUtgaaendeJournalpost,
        Command::OpprettInterntNotatJournalpost(_) => {
            CommandTypeCode::OpprettInterntNotatJournalpost
        }
        Command::AvsluttSak(_) => CommandTypeCode::AvsluttSak,
        Command::SettSaksansvarlig(_) => CommandTypeCode::SettSaksansvarlig,
    }
}

pub fn kontekst(command: &Command) -> Statuskontekst {
    let mut kontekst = Statuskontekst::default();

    match command {
        Command::OpprettSak(inner) => {
            kontekst.sak_client_reference = Some(inner.client_reference.to_string());
        }
        Command::AvsluttSak(inner) => sak_key_kontekst(&mut kontekst, &inner.sak_key),
        Command::SettSaksansvarlig(inner) => sak_key_kontekst(&mut kontekst, &inner.sak_key),
        Command::OpprettInngaaendeJournalpost(inner)
        | Command::OpprettUtgaaendeJournalpost(inner)
        | Command::OpprettInterntNotatJournalpost(inner) => {
            let felles = inner.felles();
            sak_key_kontekst(&mut kontekst, &felles.sak_key);
            kontekst.journalpost_client_reference = Some(felles.client_reference.to_string());
            kontekst.dokument_client_references = felles
                .dokumenter
                .iter()
                .map(|dokument| dokument.client_reference.to_string())
                .collect();
        }
    }

    kontekst
}

fn sak_key_kontekst(kontekst: &mut Statuskontekst, sak_key: &SakKey) {
    match sak_key {
        SakKey::ClientReference(client_reference) => {
            kontekst.sak_client_reference = Some(client_reference.to_string());
        }
        SakKey::ArkivId(arkiv_id) => {
            kontekst.saksnummer = Some(arkiv_id.clone());
        }
    }
}
