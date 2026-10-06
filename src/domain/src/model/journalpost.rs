use chrono::NaiveDateTime;
use uuid::Uuid;

use crate::model::{dokument::Dokument, tilgang::Tilgang};

#[allow(dead_code)]
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct JournalpostId(pub String);

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Journalpost {
    pub client_reference: Option<Uuid>,
    pub tittel: String,
    pub dokument_dato: NaiveDateTime,
    pub journalposttype: JournalpostType,
    pub journalstatus: Journalpoststatus,
    pub tilgang: Option<Tilgang>,

    pub saksbehandler: Option<String>,
    pub saksbehandler_enhet: Option<String>,
    /// Hoveddokumentet først, deretter vedlegg.
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
