use async_trait::async_trait;
use chrono::NaiveDate;
use domain::model::dokument::{ArkivDokumentId, Dokument};
use domain::model::journalpost::{Journalpost, JournalpostType, Journalpoststatus};
use domain::model::sak::{Ordningsverdi, Sak, Saksbehandler, Saksnummer, Saksstatus, Sakstittel};

use application::query::services::hent_sak::SakRepository;

#[derive(Clone, Debug, Default)]
pub struct FakeSakRepository;

impl FakeSakRepository {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SakRepository for FakeSakRepository {
    async fn hent_sak(
        &self,
        saksnummer: Saksnummer,
        inkluder_journalposter: bool,
    ) -> Result<Sak, anyhow::Error> {
        let saksbehandler = Saksbehandler::new("Z00000".to_string(), "42".to_string())?;
        Ok(Sak {
            client_reference: None,
            sakstittel: Sakstittel("Fake sak".to_string()),
            saksbehandler: saksbehandler.saksbehandler_id,
            saksbehandler_enhet: Some(saksbehandler.saksbehandler_enhet),
            saksstatus: Saksstatus::UnderBehandling,
            tilgang: None,
            saksnummer,
            kildesystem: "SKUFFEN".to_string(),
            lukket: false,
            journalposter: Some(if inkluder_journalposter {
                vec![fake_journalpost()?]
            } else {
                vec![]
            }),
            ordningsverdi: Ordningsverdi::new("2026-1".to_string())?,
        })
    }
}

fn fake_journalpost() -> Result<Journalpost, anyhow::Error> {
    let dokument = |id, tittel: &str, filtype: &str| Dokument {
        dokument_id: ArkivDokumentId(id),
        client_reference: None,
        tittel: tittel.to_string(),
        filtype: filtype.to_string(),
        dokument_referanse: None,
    };
    Ok(Journalpost {
        client_reference: None,
        tittel: "Fake journalpost".to_string(),
        dokument_dato: NaiveDate::from_ymd_opt(2025, 10, 14)
            .and_then(|dato| dato.and_hms_opt(0, 0, 0))
            .ok_or_else(|| anyhow::anyhow!("ugyldig fake-dato"))?,
        journalposttype: JournalpostType::Inngående,
        journalstatus: Journalpoststatus::Registrert,
        tilgang: None,
        saksbehandler: None,
        saksbehandler_enhet: None,
        dokumenter: vec![
            dokument(20_001, "Fake hoveddokument", "PDF"),
            dokument(20_002, "Fake vedlegg", "TXT"),
        ],
        journalpost_id: 10_001,
        kildesystem: None,
    })
}
