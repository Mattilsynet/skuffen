//! Statuskontrakten innad (SKU-0016 R8/R9, D31–D33).
//!
//! Én strøm, to hendelsestyper. Dagens 3×5-matrise av `phase` × `status` med
//! `unreachable!()` for ulovlige kombinasjoner er erstattet av flate,
//! uttømmende enums.

use chrono::Utc;
use uuid::Uuid;

use crate::eksekvering::operasjon::{OperasjonId, Operasjonstype};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTypeCode {
    OpprettSak,
    OpprettInngaaendeJournalpost,
    OpprettUtgaaendeJournalpost,
    OpprettInterntNotatJournalpost,
    AvsluttSak,
    SettSaksansvarlig,
}

impl CommandTypeCode {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::OpprettSak => "opprett_sak",
            Self::OpprettInngaaendeJournalpost => "opprett_inngaaende_journalpost",
            Self::OpprettUtgaaendeJournalpost => "opprett_utgaaende_journalpost",
            Self::OpprettInterntNotatJournalpost => "opprett_internt_notat_journalpost",
            Self::AvsluttSak => "avslutt_sak",
            Self::SettSaksansvarlig => "sett_saksansvarlig",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let value = match code {
            "opprett_sak" => Self::OpprettSak,
            "opprett_inngaaende_journalpost" => Self::OpprettInngaaendeJournalpost,
            "opprett_utgaaende_journalpost" => Self::OpprettUtgaaendeJournalpost,
            "opprett_internt_notat_journalpost" => Self::OpprettInterntNotatJournalpost,
            "avslutt_sak" => Self::AvsluttSak,
            "sett_saksansvarlig" => Self::SettSaksansvarlig,
            _ => return None,
        };
        Some(value)
    }
}

/// Klientvennlige, bevisst grovkornede feilkoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusErrorCode {
    InvalidRequest,
    NotFound,
    Conflict,
    PrerequisitePending,
    TemporaryUnavailable,
    ProcessingFailed,
}

impl StatusErrorCode {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::PrerequisitePending => "prerequisite_pending",
            Self::TemporaryUnavailable => "temporary_unavailable",
            Self::ProcessingFailed => "processing_failed",
        }
    }
}

// ---------------------------------------------------------------------------
// CommandEventr
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEvent {
    Mottatt,
    Validert,
    Avvist,
    Utfores,
    Fullfort,
    Feilet,
}

impl CommandEvent {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Mottatt => "mottatt",
            Self::Validert => "validert",
            Self::Avvist => "avvist",
            Self::Utfores => "utfores",
            Self::Fullfort => "fullfort",
            Self::Feilet => "feilet",
        }
    }

    /// `terminal: true` betyr at utfallet er avgjort, ikke at flere eventer er
    /// utelukket (SKU-0016 R9). Operasjonseventer kan fortsette etterpå fordi
    /// søsken kjører videre best effort.
    pub fn er_terminal(self) -> bool {
        matches!(self, Self::Avvist | Self::Fullfort | Self::Feilet)
    }
}

// ---------------------------------------------------------------------------
// Operasjonshendelser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operasjonshendelse {
    /// Recoverable feil; nytt forsøk kommer.
    ForsokFeilet,
    Ok,
    Feilet,
    /// Ukjent utfall etter crash i `sendt`. Krever menneske (SKU-0016 R5).
    KreverAvklaring,
    /// Advisory 24-timersvarsel. Avbryter ingenting (D11).
    Varsel,
}

impl Operasjonshendelse {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::ForsokFeilet => "forsok_feilet",
            Self::Ok => "ok",
            Self::Feilet => "feilet",
            Self::KreverAvklaring => "krever_avklaring",
            Self::Varsel => "varsel",
        }
    }

    pub fn er_terminal(self) -> bool {
        matches!(self, Self::Ok | Self::Feilet)
    }
}

// ---------------------------------------------------------------------------
// Kontekst og hendelser
// ---------------------------------------------------------------------------

/// Identifikatorer klienten kan koble mot sine egne referanser.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Statuskontekst {
    pub sak_client_reference: Option<String>,
    pub saksnummer: Option<String>,
    pub journalpost_client_reference: Option<String>,
    pub journalpost_arkiv_id: Option<String>,
    pub dokument_client_references: Vec<String>,
}

impl Statuskontekst {
    pub fn is_empty(&self) -> bool {
        self.sak_client_reference.is_none()
            && self.saksnummer.is_none()
            && self.journalpost_client_reference.is_none()
            && self.journalpost_arkiv_id.is_none()
            && self.dokument_client_references.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandStatus {
    pub command_id: Uuid,
    pub correlation_id: Option<Uuid>,
    pub command_type: CommandTypeCode,
    pub hendelse: CommandEvent,
    pub terminal: bool,
    /// Statisk, klientvennlig. Aldri intern detalj eller stacktrace.
    pub melding: String,
    pub error_code: Option<StatusErrorCode>,
    pub kontekst: Statuskontekst,
    pub timestamp: String,
}

impl CommandStatus {
    pub fn new(
        command_id: Uuid,
        correlation_id: Option<Uuid>,
        command_type: CommandTypeCode,
        hendelse: CommandEvent,
        melding: impl Into<String>,
        error_code: Option<StatusErrorCode>,
        kontekst: Statuskontekst,
    ) -> Self {
        Self {
            command_id,
            correlation_id,
            command_type,
            hendelse,
            terminal: hendelse.er_terminal(),
            melding: melding.into(),
            error_code,
            kontekst,
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    /// Deduplisering bruker id-er vi allerede har i databasen (D33).
    pub fn message_id(&self) -> String {
        format!("{}:{}", self.command_id, self.hendelse.as_code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operasjonstatus {
    pub command_id: Uuid,
    pub correlation_id: Option<Uuid>,
    pub operasjon_id: OperasjonId,
    pub operasjonstype: Operasjonstype,
    pub hendelse: Operasjonshendelse,
    pub attempt_no: i32,
    pub terminal: bool,
    pub melding: String,
    pub error_code: Option<StatusErrorCode>,
    pub timestamp: String,
}

impl Operasjonstatus {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command_id: Uuid,
        correlation_id: Option<Uuid>,
        operasjon_id: OperasjonId,
        operasjonstype: Operasjonstype,
        hendelse: Operasjonshendelse,
        attempt_no: i32,
        melding: impl Into<String>,
        error_code: Option<StatusErrorCode>,
    ) -> Self {
        Self {
            command_id,
            correlation_id,
            operasjon_id,
            operasjonstype,
            hendelse,
            attempt_no,
            terminal: hendelse.er_terminal(),
            melding: melding.into(),
            error_code,
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    /// Publiseres kun ved forsøksutfall, aldri ved `blokkert ↔ klar`-flakking
    /// (D33). Blokkeringsårsak er spørrbar tilstand, ikke en hendelse.
    pub fn message_id(&self) -> String {
        format!("{}:{}", self.operasjon_id.0, self.attempt_no)
    }
}

// ---------------------------------------------------------------------------
// Eksekveringsfeil
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feiltype {
    /// Retryes for alltid med backoff (SKU-0016 R6).
    Recoverable,
    /// Terminal `feilet`.
    Irrecoverable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EksekveringFeil {
    pub feiltype: Feiltype,
    pub melding: String,
}

impl EksekveringFeil {
    pub fn recoverable(melding: impl Into<String>) -> Self {
        Self {
            feiltype: Feiltype::Recoverable,
            melding: melding.into(),
        }
    }

    pub fn irrecoverable(melding: impl Into<String>) -> Self {
        Self {
            feiltype: Feiltype::Irrecoverable,
            melding: melding.into(),
        }
    }

    pub fn er_recoverable(&self) -> bool {
        self.feiltype == Feiltype::Recoverable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_type_koder_er_rundturssikre() {
        for code in [
            CommandTypeCode::OpprettSak,
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTypeCode::OpprettInterntNotatJournalpost,
            CommandTypeCode::AvsluttSak,
            CommandTypeCode::SettSaksansvarlig,
        ] {
            assert_eq!(CommandTypeCode::from_code(code.as_code()), Some(code));
        }
    }

    #[test]
    fn kun_avgjorte_utfall_er_terminale() {
        assert!(CommandEvent::Fullfort.er_terminal());
        assert!(CommandEvent::Feilet.er_terminal());
        assert!(CommandEvent::Avvist.er_terminal());
        assert!(!CommandEvent::Mottatt.er_terminal());
        assert!(!CommandEvent::Validert.er_terminal());
        assert!(!CommandEvent::Utfores.er_terminal());
    }

    #[test]
    fn varsel_er_ikke_terminalt() {
        assert!(!Operasjonshendelse::Varsel.er_terminal());
        assert!(!Operasjonshendelse::ForsokFeilet.er_terminal());
        assert!(!Operasjonshendelse::KreverAvklaring.er_terminal());
    }

    #[test]
    fn message_id_bruker_id_er_vi_allerede_har() {
        let command_id = Uuid::from_u128(1);
        let command = CommandStatus::new(
            command_id,
            None,
            CommandTypeCode::OpprettSak,
            CommandEvent::Fullfort,
            "Ferdig.",
            None,
            Statuskontekst::default(),
        );
        assert_eq!(command.message_id(), format!("{command_id}:fullfort"));

        let operasjon_id = OperasjonId(Uuid::from_u128(2));
        let operasjon = Operasjonstatus::new(
            command_id,
            None,
            operasjon_id,
            Operasjonstype::OpprettSak,
            Operasjonshendelse::ForsokFeilet,
            3,
            "Midlertidig feil.",
            Some(StatusErrorCode::TemporaryUnavailable),
        );
        assert_eq!(operasjon.message_id(), format!("{}:3", operasjon_id.0));
    }
}
