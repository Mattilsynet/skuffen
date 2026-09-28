use uuid::Uuid;

/// Arkivets identitet for et dokument.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct ArkivDokumentId(pub i32);

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Dokument {
    pub dokument_id: ArkivDokumentId,
    pub client_reference: Option<Uuid>,
    pub tittel: String,
    pub filtype: String,
    pub dokument_referanse: Option<Uuid>,
}
