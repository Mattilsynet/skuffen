use lib_schemas::skuffen::query::responses::DokumentResponse;

pub fn from_domain_dokument_to_dto(
    domain_dokument: domain::model::dokument::Dokument,
) -> DokumentResponse {
    DokumentResponse {
        tittel: domain_dokument.tittel,
        filtype: domain_dokument.filtype,
        dokument_referanse: domain_dokument.dokument_referanse,
    }
}
