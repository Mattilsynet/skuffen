//! Statuskontrakten innad (SKU-0016 R8/R9, D31–D33).
//!
//! Én strøm, to hendelsestyper. Erstatter matrisen `phase` × `status`, som
//! hadde kombinasjoner som aldri kunne oppstå.

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
// Kommandohendelser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEvent {
    Mottatt,
    Validert,
    Avvist,
    Utfores,
    Fullfort,
    Feilet,
    /// Minst én operasjon har ukjent utfall og må avklares manuelt.
    ///
    /// Bevisst ikke terminal: utfallet *er* ikke avgjort, og operasjonen kan
    /// bli `ok` etter admin write. Monotonien i foldet må bevares.
    KreverAvklaring,
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
            Self::KreverAvklaring => "krever_avklaring",
        }
    }

    /// Utfallet er avgjort (SKU-0016 R9). Operasjonseventer kan fortsette
    /// etterpå, fordi søsken kjører videre best effort.
    pub fn er_terminal(self) -> bool {
        matches!(self, Self::Avvist | Self::Fullfort | Self::Feilet)
    }
}

// ---------------------------------------------------------------------------
// Operasjonshendelser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operasjonshendelse {
    /// Nytt forsøk kommer.
    ForsokFeilet,
    Ok,
    Feilet,
    /// Ukjent utfall etter crash i `sendt`. Krever menneske (SKU-0016 R5).
    KreverAvklaring,
    /// Advisory 24-timersvarsel (D11).
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

/// Identifikatorer klienten kan koble mot sine egne.
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
    /// Klientvennlig. Aldri intern detalj eller stacktrace.
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
}

// ---------------------------------------------------------------------------
// Eksekveringsfeil
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feiltype {
    /// Retryes for alltid med backoff (SKU-0016 R6).
    Recoverable,
    Irrecoverable,
}

/// En feil under eksekvering, med alt de tre konsumentene trenger.
///
/// Feltene har hver sin mottaker, og de blandes aldri:
///
/// - `kode` er stabil og greppbar, og går til `operasjon.siste_detalj`.
/// - `melding` og `error_code` går til klienten i statusstrømmen.
/// - `intern_detalj` går **kun** til `siste_detalj`. Her hører underliggende
///   feiltekst hjemme — sqlx-feil og lignende som ingen andre logger, men som
///   klienten ikke skal se.
///
/// Adapteren som konstruerer feilen bestemmer alle fire; executoren
/// videreformidler dem uten å tolke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EksekveringFeil {
    pub feiltype: Feiltype,
    pub kode: &'static str,
    pub melding: String,
    pub error_code: StatusErrorCode,
    pub intern_detalj: Option<String>,
}

/// Klienten skal ikke se innsiden av Skuffen.
const INTERN_MELDING: &str = "Intern feil i behandlingen.";
const INTERN_MIDLERTIDIG_MELDING: &str = "Midlertidig intern feil. Nytt forsøk kommer.";

impl EksekveringFeil {
    pub fn recoverable(
        kode: &'static str,
        melding: impl Into<String>,
        error_code: StatusErrorCode,
    ) -> Self {
        Self {
            feiltype: Feiltype::Recoverable,
            kode,
            melding: melding.into(),
            error_code,
            intern_detalj: None,
        }
    }

    pub fn irrecoverable(
        kode: &'static str,
        melding: impl Into<String>,
        error_code: StatusErrorCode,
    ) -> Self {
        Self {
            feiltype: Feiltype::Irrecoverable,
            kode,
            melding: melding.into(),
            error_code,
            intern_detalj: None,
        }
    }

    /// Feil i Skuffen selv, ikke i arkivet. Koden bærer detaljen for logg og
    /// `siste_detalj`; klienten får en generisk tekst fordi det ikke er noe
    /// den kan rette.
    pub fn intern(kode: &'static str) -> Self {
        Self::irrecoverable(kode, INTERN_MELDING, StatusErrorCode::ProcessingFailed)
    }

    /// Intern feil som går over av seg selv — typisk en databasehikke.
    pub fn intern_midlertidig(kode: &'static str) -> Self {
        Self::recoverable(
            kode,
            INTERN_MIDLERTIDIG_MELDING,
            StatusErrorCode::TemporaryUnavailable,
        )
    }

    /// Underliggende feiltekst for `siste_detalj`. Går aldri til klienten.
    pub fn med_intern_detalj(mut self, detalj: impl Into<String>) -> Self {
        self.intern_detalj = Some(detalj.into());
        self
    }

    /// Det som skrives til `operasjon.siste_detalj`. Koden først, så den er
    /// greppbar med prefiks selv når en detalj henger på.
    pub fn siste_detalj(&self) -> String {
        match &self.intern_detalj {
            Some(detalj) => format!("{} {detalj}", self.kode),
            None => self.kode.to_string(),
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
    fn interne_feil_lekker_ikke_detaljen_til_klienten() {
        let feil = EksekveringFeil::intern("intern_sak_attributter_mangler");

        // Detaljen er greppbar i koden, som går til siste_detalj ...
        assert_eq!(feil.kode, "intern_sak_attributter_mangler");
        // ... men klienten får ingen innsikt i Skuffens innside.
        assert_eq!(feil.melding, "Intern feil i behandlingen.");
        assert_eq!(feil.error_code, StatusErrorCode::ProcessingFailed);
        assert!(!feil.er_recoverable());
    }

    #[test]
    fn intern_midlertidig_er_recoverable() {
        let feil = EksekveringFeil::intern_midlertidig("intern_fakta_utilgjengelig");

        assert!(feil.er_recoverable());
        assert_eq!(feil.error_code, StatusErrorCode::TemporaryUnavailable);
    }

    #[test]
    fn intern_detalj_gaar_til_siste_detalj_men_aldri_til_klienten() {
        let feil = EksekveringFeil::intern_midlertidig("intern_fakta_utilgjengelig")
            .med_intern_detalj("pool timed out while connecting");

        assert_eq!(
            feil.siste_detalj(),
            "intern_fakta_utilgjengelig pool timed out while connecting"
        );
        // Klientmeldingen er urørt av detaljen.
        assert_eq!(feil.melding, "Midlertidig intern feil. Nytt forsøk kommer.");
    }

    #[test]
    fn siste_detalj_er_bare_koden_uten_detalj() {
        // Arkivfeil trenger ingen detalj: sikri_client har allerede logget
        // status, endepunkt og body. Da skal siste_detalj være ren og
        // greppbar.
        let feil = EksekveringFeil::irrecoverable(
            "sikri_unknown_user",
            "Ugyldig saksbehandler.",
            StatusErrorCode::InvalidRequest,
        );

        assert_eq!(feil.siste_detalj(), "sikri_unknown_user");
    }
}
