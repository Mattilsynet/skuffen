use anyhow::{anyhow, Result};
use sikri_client::domain::sak::SakRespons as SikriSak;
use tracing::info;

use crate::mapping::fra_sikri_til_domene::journalpost::from_sikri_journalpost_to_domain_journalpost;

pub fn from_sikri_sak_to_domain_sak(sikri_sak: SikriSak) -> Result<domain::model::sak::Sak> {
    let sak_response = domain::model::sak::Sak {
        sakstittel: sikri_sak.sakstittel,
        saksbehandler: sikri_sak
            .saksbehandler
            .ok_or_else(|| anyhow!("Sak har ikke saksbehandler"))?,
        saksstatus: saksstatus_from_char(
            sikri_sak
                .saksstatus
                .ok_or_else(|| anyhow!("Sak har ikke saksstatus."))?
                .chars()
                .next()
                .ok_or_else(|| anyhow!("Saksstatus string har ingen characters."))?,
        )?,
        unntatt_offentlighet: true, //TODO: Satt til true bare for å teste. FIX
        saksnr: domain::model::sak::Saksnummer::new(sikri_sak.saksnr.unwrap_or_default())?,
        lukket: sikri_sak.lukket,
        kildesystem: "SKUFFEN".to_string(),
        journalposter: sikri_sak
            .journalposter
            .unwrap_or_else(|| Vec::new())
            .iter()
            .map(|jp| from_sikri_journalpost_to_domain_journalpost(jp.clone()))
            .collect::<Result<Vec<domain::model::journalpost::Journalpost>>>()?,
    };
    info!("{:?}", sak_response);
    Ok(sak_response)
}

fn saksstatus_from_char(c: char) -> Result<domain::model::sak::Saksstatus> {
    let saksstatus = match c {
        'B' => domain::model::sak::Saksstatus::UnderBehandling,
        'F' => domain::model::sak::Saksstatus::Ferdig,
        'A' => domain::model::sak::Saksstatus::Avsluttet,
        _ => {
            return Err(anyhow::anyhow!("Ukjent saksstatus: {c}"));
        }
    };
    Ok(saksstatus)
}
