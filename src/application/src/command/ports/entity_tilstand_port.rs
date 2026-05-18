use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    DokumentTilstand, JournalpostTilstand, SakMedBarn, SakTilstand,
};
use lib_schemas::skuffen::dokument::Felt;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
#[async_trait]
pub trait EntityTilstandRepository: Send + Sync {
    // Sak
    async fn opprett_sak_tilstand(
        &self,
        sak_id: SkuffenSakId,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_sak_tilstand(
        &self,
        sak_id: SkuffenSakId,
        tilstand: SakTilstand,
        sikri_id: Option<i64>,
        saksnummer: Option<&str>,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_oensket_saksansvarlig(
        &self,
        sak_id: SkuffenSakId,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_naavaerende_saksansvarlig(
        &self,
        sak_id: SkuffenSakId,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error>;

    // Journalpost
    async fn opprett_journalpost_tilstand(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
        journalposttype: JournalpostType,
        med_utsending: bool,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_journalpost_tilstand(
        &self,
        journalpost_id: SkuffenJournalpostId,
        tilstand: JournalpostTilstand,
        sikri_id: Option<i64>,
        journalpostnummer: Option<i32>,
    ) -> Result<(), anyhow::Error>;

    async fn hent_sak_id_fra_journalpost_id(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error>;

    // Dokument
    async fn opprett_dokument_tilstand(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
        tilstand: DokumentTilstand,
        mal_referanse: Option<Uuid>,
        felter: Vec<Felt>,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_dokument_tilstand(
        &self,
        dokument_id: SkuffenDokumentId,
        tilstand: DokumentTilstand,
    ) -> Result<(), anyhow::Error>;

    async fn hent_journalpost_id_fra_dokument_id(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error>;

    async fn oppdater_rendered_dokument_referanse(
        &self,
        dokument_id: SkuffenDokumentId,
        rendered_dokument_referanse: Uuid,
    ) -> Result<(), anyhow::Error>;

    // Aggregat-henting
    async fn hent_sak_med_barn(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakMedBarn>, anyhow::Error>;

    // Historikk
    async fn logg_overgang(
        &self,
        entity_type: &str,
        entity_id: Uuid,
        command_id: Uuid,
        fra_tilstand: &str,
        til_tilstand: &str,
        operasjon: &str,
        feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error>;
}
