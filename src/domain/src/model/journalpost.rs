use uuid::Uuid;

use crate::model::dokument::Dokument;

#[allow(dead_code)]
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct JournalpostId(pub String);

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Journalpost {
    pub tittel: String,
    pub dokument_dato: String,
    pub journalposttype: JournalpostType,
    pub journalstatus: Journalpoststatus,
    pub unntatt_offentlighet: bool,

    pub saksbehandler: String,
    pub dokumenter: Vec<Dokument>,
    pub journalpost_id: i32,
    pub kildesystem: Option<String>,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum JournalpostKey {
    SkuffenId(Uuid),
    ArkivId(JournalpostId),
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum JournalpostType {
    Inngående,
    Utgående,
    InterntNotat,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum Journalpoststatus {
    Registrert,
    Reservert,
    Midlertidig,
    Ferdig,
    Ekspedert,
    Journalført,
}
