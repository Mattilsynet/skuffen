use chrono::{DateTime, Utc};
use uuid::Uuid;

#[allow(dead_code)]
pub struct JournalpostId(i32);

pub struct Journalpost {
    pub tittel: String,
    pub dokument_dato: DateTime<Utc>,
    pub journalposttype: JournalpostType,
    pub journalstatus: Journalpoststatus,
    pub unntatt_offentlighet: bool,

    pub saksbehandler: String,
    pub dokumenter: Option<Vec<Dokument>>,
    pub journalpost_id: i32,
    pub kildesystem: Option<String>,
}

pub struct Dokument {
    pub tittel: String,
    pub filtype: String,
    pub dokument_referanse: Uuid,
}

pub enum JournalpostType {
    Inngående,
    Utgående,
    InterntNotat,
}

pub enum Journalpoststatus {
    Registrert,
    Reservert,
    Midlertidig,
    Ferdig,
    Ekspedert,
    Journalført,
}

impl Journalpoststatus {
    pub fn code(self) -> char {
        match self {
            Journalpoststatus::Registrert => 'S',
            Journalpoststatus::Reservert => 'R',
            Journalpoststatus::Midlertidig => 'M',
            Journalpoststatus::Ferdig => 'F',
            Journalpoststatus::Ekspedert => 'E',
            Journalpoststatus::Journalført => 'J',
        }
    }

    pub fn from_code(c: char) -> Option<Self> {
        match c {
            'S' => Some(Self::Registrert),
            'R' => Some(Self::Reservert),
            'M' => Some(Self::Midlertidig),
            'F' => Some(Self::Ferdig),
            'E' => Some(Self::Ekspedert),
            'J' => Some(Self::Journalført),
            _ => None,
        }
    }
}
