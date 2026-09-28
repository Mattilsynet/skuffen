use anyhow::{Result, anyhow};
use sikri_client::domain::sak::SakRespons as SikriSak;

use crate::query::mapping::fra_sikri_til_domene::journalpost::from_sikri_journalpost_to_domain_journalpost;

pub fn from_sikri_sak_to_domain_sak(sikri_sak: SikriSak) -> Result<domain::model::sak::Sak> {
    let saksnummer = domain::model::sak::Saksnummer::new(
        sikri_sak
            .saksnr
            .clone()
            .ok_or_else(|| anyhow!("Sikri sak har ikke saksnummer."))?,
    )?;
    let tilgang = from_sikri_sak_to_domain_tilgang(&sikri_sak)?;

    let journalposter = sikri_sak
        .journalposter
        .map(|journalposter| {
            journalposter
                .into_iter()
                .map(from_sikri_journalpost_to_domain_journalpost)
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;

    Ok(domain::model::sak::Sak {
        client_reference: None,
        sakstittel: domain::model::sak::Sakstittel::try_from(sikri_sak.sakstittel)?,
        saksbehandler: sikri_sak
            .saksbehandler
            .ok_or_else(|| anyhow!("Sak har ikke saksbehandler"))?,
        saksstatus: saksstatus_from_char(
            sikri_sak
                .saksstatus
                .as_deref()
                .ok_or_else(|| anyhow!("Sak har ikke saksstatus."))?
                .chars()
                .next()
                .ok_or_else(|| anyhow!("Saksstatus string har ingen characters."))?,
        )?,
        tilgang,
        saksnummer,
        lukket: sikri_sak.lukket,
        kildesystem: "SKUFFEN".to_string(),
        journalposter,
        ordningsverdi: domain::model::sak::Ordningsverdi::new(sikri_sak.ordningsverdi)?,
    })
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
    sikri_sak: &SikriSak,
) -> Result<Option<domain::model::tilgang::Tilgang>> {
    // Fail-closed: en delvis tilgang (bare kode eller bare hjemmel) må aldri
    // tolkes som «ingen skjerming». Da avviser vi i stedet for å vise uskjermet.
    match (
        sikri_sak.tilgangskode.clone(),
        sikri_sak.tilgangshjemmel.clone(),
    ) {
        (Some(tilgangskode), Some(tilgangshjemmel)) => Ok(Some(domain::model::tilgang::Tilgang {
            tilgangskode: domain::model::tilgang::Tilgangskode::new(tilgangskode)?,
            tilgangshjemmel: domain::model::tilgang::Tilgangshjemmel::new(tilgangshjemmel)?,
        })),
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(anyhow!(
            "Sikri sak har delvis tilgang (kun kode eller kun hjemmel); avviser fail-closed"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::mapping::fra_domene_til_dto::sak::from_domain_sak_to_dto;
    use serde_json::{Value, json};
    use sikri_client::dto::elements_sak_response::ElementsSakMedJournalposterResponse;

    fn sikri_journalpost(id: i32) -> Value {
        json!({
            "journalpostId": id,
            "tittel": "Syntetisk journalpost",
            "journalposttype": "X",
            "journalstatus": "M",
            "dokumentDato": "2025-10-14T00:00:00",
            "hoveddokId": 21,
            "harHoveddokument": true,
            "antallVedlegg": 1,
            "saksbehandler": null,
            "dokumenterRespons": [
                {"dokumentId": 22, "hoveddokId": 21, "tittel": "Vedlegg", "hoveddokument": false, "filtype": "TXT", "dokumentBase64": "c2hvdWxkLW5vdC1sZWFr"},
                {"dokumentId": 21, "hoveddokId": 21, "tittel": "Hoved", "hoveddokument": true, "filtype": "PDF", "dokumentBase64": null}
            ],
            "dokumenter": null
        })
    }

    fn sikri_sak(journalposter: Option<Value>) -> Value {
        let mut sak = json!({
            "saksnr": "2026/000123",
            "sakstittel": "Syntetisk sak",
            "saksbehandler": "Z00001",
            "saksstatus": "B",
            "ordningsverdi": "430",
            "lukket": false
        });
        if let Some(journalposter) = journalposter {
            sak["journalposter"] = journalposter;
        }
        sak
    }

    fn til_respons(sak: Value) -> Result<Value> {
        let elements: ElementsSakMedJournalposterResponse = serde_json::from_value(sak)?;
        let domene = from_sikri_sak_to_domain_sak(SikriSak::from(elements))?;
        Ok(serde_json::to_value(from_domain_sak_to_dto(domene)?)?)
    }

    #[test]
    fn journalposter_og_dokumentmetadata_foeres_frem_uten_innhold() {
        let respons = til_respons(sikri_sak(Some(json!([sikri_journalpost(11)])))).unwrap();

        assert_eq!(respons["saksnummer"], "2026/000123");
        let journalpost = &respons["journalposter"][0];
        assert_eq!(journalpost["dokument_dato"], "2025-10-14T00:00:00");
        assert_eq!(journalpost["saksbehandler"], Value::Null);
        assert_eq!(
            journalpost["dokumenter"],
            json!([
                {"dokument_id": "21", "tittel": "Hoved", "filtype": "PDF", "dokument_referanse": null},
                {"dokument_id": "22", "tittel": "Vedlegg", "filtype": "TXT", "dokument_referanse": null}
            ])
        );
        assert!(!respons.to_string().contains("c2hvdWxkLW5vdC1sZWFr"));
    }

    #[test]
    fn plassholder_for_ufordelt_saksbehandler_bevares() {
        let mut journalpost = sikri_journalpost(11);
        journalpost["saksbehandler"] = json!("---");

        let respons = til_respons(sikri_sak(Some(json!([journalpost])))).unwrap();

        assert_eq!(respons["journalposter"][0]["saksbehandler"], "---");
    }

    #[test]
    fn manglende_journalpostliste_er_ikke_tom_liste() {
        let respons = til_respons(sikri_sak(None)).unwrap();

        assert!(respons.get("journalposter").is_none());
    }

    #[test]
    fn eksplisitt_tom_journalpostliste_bevares() {
        let respons = til_respons(sikri_sak(Some(json!([])))).unwrap();

        assert_eq!(respons["journalposter"], json!([]));
    }

    #[test]
    fn dokumentdato_er_obligatorisk_og_maa_ha_klokkeslett() {
        for dato in [Value::Null, json!("2025-10-14"), json!("ikke-dato")] {
            let mut journalpost = sikri_journalpost(11);
            journalpost["dokumentDato"] = dato;

            assert!(til_respons(sikri_sak(Some(json!([journalpost])))).is_err());
        }
    }

    #[test]
    fn ukjent_journalstatus_gir_kontrollert_feil() {
        let mut journalpost = sikri_journalpost(11);
        journalpost["journalstatus"] = json!("Q");

        assert!(til_respons(sikri_sak(Some(json!([journalpost])))).is_err());
    }

    #[test]
    fn delvis_skjerming_paa_journalpost_avvises() {
        let mut journalpost = sikri_journalpost(11);
        journalpost["tilgangskode"] = json!("UO");

        assert!(til_respons(sikri_sak(Some(json!([journalpost])))).is_err());
    }
}
