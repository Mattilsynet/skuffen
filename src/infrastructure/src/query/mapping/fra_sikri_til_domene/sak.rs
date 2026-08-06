use anyhow::{Result, anyhow};
use sikri_client::domain::sak::SakRespons as SikriSak;

use crate::query::mapping::{
    fra_sikri_til_domene::journalpost::from_sikri_journalpost_to_domain_journalpost,
    lookup::key_mapping_queries::lookup_skuffen_id_fra_arkiv_id,
};

pub async fn from_sikri_sak_to_domain_sak(sikri_sak: SikriSak) -> Result<domain::model::sak::Sak> {
    let saksnummer_str = sikri_sak
        .saksnr
        .clone()
        .ok_or_else(|| anyhow!("Sikri sak har ikke saksnummer."))?;

    let client_reference = None;

    let mut journalposter = Vec::new();
    if let Some(ref sikri_journalposter) = sikri_sak.journalposter {
        for jp in sikri_journalposter {
            journalposter.push(from_sikri_journalpost_to_domain_journalpost(jp.clone()).await?);
        }
    }

    let sak_response = domain::model::sak::Sak {
        client_reference,
        sakstittel: domain::model::sak::Sakstittel::try_from(sikri_sak.sakstittel.clone())?,
        saksbehandler: sikri_sak
            .saksbehandler
            .clone()
            .ok_or_else(|| anyhow!("Sak har ikke saksbehandler"))?,
        saksstatus: saksstatus_from_char(
            sikri_sak
                .saksstatus
                .clone()
                .ok_or_else(|| anyhow!("Sak har ikke saksstatus."))?
                .chars()
                .next()
                .ok_or_else(|| anyhow!("Saksstatus string har ingen characters."))?,
        )?,
        tilgang: from_sikri_sak_to_domain_tilgang(sikri_sak.clone())?,
        sak_key: from_sikri_saksnummer_to_domain_sak_key(saksnummer_str).await?,
        lukket: sikri_sak.lukket,
        kildesystem: "SKUFFEN".to_string(),
        journalposter,
        ordningsverdi: domain::model::sak::Ordningsverdi::new(sikri_sak.ordningsverdi)?,
    };
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

fn from_sikri_sak_to_domain_tilgang(
    sikri_sak: SikriSak,
) -> Result<Option<domain::model::tilgang::Tilgang>> {
    // Fail-closed: en delvis tilgang (bare kode eller bare hjemmel) må aldri
    // tolkes som «ingen skjerming». Da avviser vi i stedet for å vise uskjermet.
    match (sikri_sak.tilgangskode, sikri_sak.tilgangshjemmel) {
        (Some(tilgangskode), Some(tilgangshjemmel)) => Ok(Some(domain::model::tilgang::Tilgang {
            tilgangskode,
            tilgangshjemmel,
        })),
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(anyhow!(
            "Sikri sak har delvis tilgang (kun kode eller kun hjemmel); avviser fail-closed"
        )),
    }
}

async fn from_sikri_saksnummer_to_domain_sak_key(
    sikri_saksnummer: String,
) -> Result<domain::model::sak::SakKey> {
    let saksnummer = domain::model::sak::Saksnummer::new(sikri_saksnummer)?;
    Ok(domain::model::sak::SakKey {
        skuffen_id: lookup_skuffen_id_fra_arkiv_id(saksnummer).await?,
    })
}
