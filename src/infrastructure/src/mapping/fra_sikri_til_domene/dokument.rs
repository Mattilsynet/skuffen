use anyhow::{Result, anyhow};
use sikri_client::domain::dokument_response::DokumentRespons as SikriDokumentResponse;

pub async fn from_sikri_dokument_to_domain_dokument(
    sikri_dokument: SikriDokumentResponse,
) -> Result<domain::model::dokument::Dokument> {
    let _dokument_id = sikri_dokument
        .dokument_id
        .ok_or_else(|| anyhow!("Dokument har ikke dokument id."))?;

    let client_reference = None;

    let domain_dokument = domain::model::dokument::Dokument {
        client_reference,
        tittel: sikri_dokument
            .tittel
            .ok_or_else(|| anyhow!("Dokument har ikke tittel."))?,
        filtype: sikri_dokument
            .filtype
            .ok_or_else(|| anyhow!("Dokument har ikke filtype."))?,
        dokument_referanse: None,
    };
    Ok(domain_dokument)
}
