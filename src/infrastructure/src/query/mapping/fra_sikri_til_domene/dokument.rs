use anyhow::{Result, anyhow};
use domain::model::dokument::{ArkivDokumentId, Dokument};
use sikri_client::domain::dokument_response::DokumentRespons as SikriDokumentResponse;

/// Mapper dokumentene og plasserer hoveddokumentet først, slik Skuffens
/// grensesnitt forventer. Et dokument regnes som hoveddokument når det er
/// markert som det, eller når ID-en er journalpostens hoveddokument-ID.
pub fn from_sikri_dokumenter_to_domain(
    sikri_dokumenter: Vec<SikriDokumentResponse>,
    hoveddok_id: Option<i32>,
    har_hoveddokument: bool,
) -> Result<Vec<Dokument>> {
    let mut hoveddokumenter = Vec::new();
    let mut vedlegg = Vec::new();
    for sikri_dokument in sikri_dokumenter {
        let er_hoveddokument = sikri_dokument.hoveddokument == Some(true)
            || (hoveddok_id.is_some() && sikri_dokument.dokument_id == hoveddok_id);
        let dokument = from_sikri_dokument_to_domain_dokument(sikri_dokument)?;
        if er_hoveddokument {
            hoveddokumenter.push(dokument);
        } else {
            vedlegg.push(dokument);
        }
    }

    if hoveddokumenter.is_empty() && vedlegg.is_empty() && !har_hoveddokument {
        return Ok(Vec::new());
    }
    if hoveddokumenter.len() != 1 {
        return Err(anyhow!(
            "Journalpostens hoveddokument kan ikke fastslås entydig ({} kandidater).",
            hoveddokumenter.len()
        ));
    }

    hoveddokumenter.extend(vedlegg);
    Ok(hoveddokumenter)
}

fn from_sikri_dokument_to_domain_dokument(
    sikri_dokument: SikriDokumentResponse,
) -> Result<Dokument> {
    Ok(Dokument {
        dokument_id: ArkivDokumentId(
            sikri_dokument
                .dokument_id
                .ok_or_else(|| anyhow!("Dokument har ikke dokument id."))?,
        ),
        client_reference: None,
        tittel: sikri_dokument
            .tittel
            .ok_or_else(|| anyhow!("Dokument har ikke tittel."))?,
        filtype: sikri_dokument
            .filtype
            .ok_or_else(|| anyhow!("Dokument har ikke filtype."))?,
        dokument_referanse: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sikri_dokument(id: i32, hoveddokument: Option<bool>) -> SikriDokumentResponse {
        SikriDokumentResponse {
            dokument_id: Some(id),
            tittel: Some(format!("Dokument {id}")),
            filtype: Some("PDF".to_string()),
            url: None,
            hoveddokument,
        }
    }

    fn ider(dokumenter: &[Dokument]) -> Vec<i32> {
        dokumenter.iter().map(|d| d.dokument_id.0).collect()
    }

    #[test]
    fn hoveddokument_flyttes_foerst_og_vedleggenes_rekkefoelge_bevares() {
        let dokumenter = from_sikri_dokumenter_to_domain(
            vec![
                sikri_dokument(3, Some(false)),
                sikri_dokument(1, Some(true)),
                sikri_dokument(2, Some(false)),
            ],
            Some(1),
            true,
        )
        .unwrap();

        assert_eq!(ider(&dokumenter), vec![1, 3, 2]);
    }

    #[test]
    fn hoveddokument_id_brukes_naar_markering_mangler() {
        let dokumenter = from_sikri_dokumenter_to_domain(
            vec![sikri_dokument(2, None), sikri_dokument(1, None)],
            Some(1),
            true,
        )
        .unwrap();

        assert_eq!(ider(&dokumenter), vec![1, 2]);
    }

    #[test]
    fn tom_dokumentliste_uten_hoveddokument_er_tom() {
        assert!(
            from_sikri_dokumenter_to_domain(vec![], None, false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn manglende_eller_motstridende_hoveddokument_gir_feil() {
        assert!(from_sikri_dokumenter_to_domain(vec![], None, true).is_err());
        assert!(
            from_sikri_dokumenter_to_domain(vec![sikri_dokument(2, Some(false))], None, true)
                .is_err()
        );
        assert!(
            from_sikri_dokumenter_to_domain(
                vec![
                    sikri_dokument(1, Some(true)),
                    sikri_dokument(2, Some(false))
                ],
                Some(2),
                true,
            )
            .is_err()
        );
    }

    #[test]
    fn dokument_uten_id_gir_feil() {
        let mut dokument = sikri_dokument(1, Some(true));
        dokument.dokument_id = None;

        assert!(from_sikri_dokumenter_to_domain(vec![dokument], None, true).is_err());
    }
}
