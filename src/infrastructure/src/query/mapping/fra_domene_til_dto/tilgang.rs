use lib_schemas::skuffen::query::responses::TilgjengelighetResponse;

pub fn from_domain_tilgang_to_tilgjengelighet(
    tilgang: Option<domain::model::tilgang::Tilgang>,
) -> TilgjengelighetResponse {
    match tilgang {
        Some(t) => TilgjengelighetResponse::Skjermet {
            tilgangskode: t.tilgangskode.as_str().to_string(),
            tilgangshjemmel: t.tilgangshjemmel.as_str().to_string(),
        },
        None => TilgjengelighetResponse::Offentlig,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SKU-0015 R7: query-svar maskeres ikke; skjermet sak/journalpost
    // rapporterer kode og hjemmel uendret til den interne, betrodde klienten.
    #[test]
    fn skjermet_speiles_uten_maskering() {
        let tilgang = domain::model::tilgang::Tilgang {
            tilgangskode: domain::model::tilgang::Tilgangskode::new("UO").unwrap(),
            tilgangshjemmel: domain::model::tilgang::Tilgangshjemmel::new("Offl. § 13").unwrap(),
        };
        let respons = from_domain_tilgang_to_tilgjengelighet(Some(tilgang));
        assert_eq!(
            respons,
            TilgjengelighetResponse::Skjermet {
                tilgangskode: "UO".to_string(),
                tilgangshjemmel: "Offl. § 13".to_string(),
            }
        );
    }

    #[test]
    fn ingen_tilgang_gir_offentlig() {
        assert_eq!(
            from_domain_tilgang_to_tilgjengelighet(None),
            TilgjengelighetResponse::Offentlig
        );
    }
}
