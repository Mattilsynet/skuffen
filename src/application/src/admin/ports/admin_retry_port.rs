use async_trait::async_trait;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::Operasjonsstatus;
use domain::eksekvering::typer::CommandTypeCode;
use uuid::Uuid;

use crate::command::{Command, CommandEnvelope};

/// Det lokale grunnlaget for å avgjøre hvor en kommando skal inn igjen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryGrunnlag {
    pub command_id: Uuid,
    pub command_type: CommandTypeCode,
    /// `None` når saken ikke var kjent ved mottak, eller raden er eldre enn
    /// koblingen.
    pub sak_id: Option<SkuffenSakId>,
    pub dekomponert: bool,
    pub operasjoner: Vec<Operasjonsstatus>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrigivelseFeil {
    #[error("sak har operasjoner med uavklart utfall")]
    SakHarUavklartUtfall(Vec<SkuffenSakId>),
    /// En operasjon på saken er låst av pågående arbeid. Admin venter ikke,
    /// så en executor aldri kan bli offer for en vranglås med et admin-kall.
    #[error("saken er opptatt")]
    Opptatt,
    #[error("admin retry repository failed")]
    Repository(#[from] anyhow::Error),
}

#[async_trait]
pub trait AdminRetryRepository: Send + Sync {
    /// `None` betyr ukjent sak; `Some(vec![])` betyr kjent sak uten kommandoer.
    /// Returnerer maksimalt `grense` command-id-er for saken.
    async fn hent_commands_for_sak(
        &self,
        key: crate::admin::model::AdminSakNokkel,
        grense: usize,
    ) -> anyhow::Result<Option<Vec<Uuid>>>;

    /// Kommandoer som ikke finnes, er utelatt.
    async fn hent_grunnlag(
        &self,
        command_ids: &[Uuid],
    ) -> Result<Vec<RetryGrunnlag>, anyhow::Error>;

    /// Atomisk mot executor og recovery: låser alle operasjoner på sakene,
    /// avviser hvis noen har uavklart utfall, og frigir deretter stoppet
    /// arbeid for de valgte kommandoene. Avvises forespørselen, endres ingenting.
    async fn frigi(
        &self,
        saker: &[SkuffenSakId],
        command_ids: &[Uuid],
    ) -> Result<(), FrigivelseFeil>;
}

/// Kommandoen slik klienten sendte den. Leses, publiseres ikke på nytt.
#[async_trait]
pub trait OriginalCommandKilde: Send + Sync {
    /// `None` når originalen ikke lenger finnes.
    async fn hent(
        &self,
        command_id: Uuid,
        command_type: CommandTypeCode,
    ) -> Result<Option<CommandEnvelope<Command>>, anyhow::Error>;
}
