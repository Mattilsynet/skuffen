use lib_schemas::skuffen::dokument::DokumentId;
use lib_schemas::skuffen::query::responses::DokumentResponse;

pub fn from_domain_dokument_to_dto(
    domain_dokument: domain::model::dokument::Dokument,
) -> DokumentResponse {
    DokumentResponse {
        dokument_id: DokumentId(domain_dokument.dokument_id.0.to_string()),
        tittel: domain_dokument.tittel,
        filtype: domain_dokument.filtype,
        dokument_referanse: domain_dokument.dokument_referanse,
    }
}
