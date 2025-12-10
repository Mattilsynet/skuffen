use uuid::Uuid;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Dokument {
    pub tittel: String,
    pub filtype: String,
    pub dokument_referanse: Option<Uuid>,
}
