use anyhow::{Result, anyhow};
use sikri_client::domain::dokument_response::DokumentRespons as SikriDokumentResponse;

pub fn from_sikri_dokument_to_domain_dokument(
    sikri_dokument: SikriDokumentResponse,
) -> Result<domain::model::dokument::Dokument> {
    let domain_dokument = domain::model::dokument::Dokument {
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
