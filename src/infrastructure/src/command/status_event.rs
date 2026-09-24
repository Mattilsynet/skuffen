//! Oversetter interne statushendelser til den utadgående wire-kontrakten.
//!
//! Meldingene skal være sanitiserte: statiske, klientvennlige strenger, aldri
//! intern `detalj` eller stacktrace.

use domain::eksekvering::operasjon::Operasjonstype;
use domain::eksekvering::typer::{
    CommandEvent, CommandStatus, Operasjonshendelse, Operasjonstatus, StatusErrorCode,
};
use lib_schemas::skuffen::journalpost::JournalpostId;
use lib_schemas::skuffen::sak::Saksnummer;
use lib_schemas::skuffen::status::{
    SkuffenCommandEvent, SkuffenCommandStatusV1, SkuffenOperasjonHendelse,
    SkuffenOperasjonStatusV1, SkuffenOperasjonstype, SkuffenStatusErrorCode,
};
use uuid::Uuid;

fn error_code(error_code: StatusErrorCode) -> SkuffenStatusErrorCode {
    match error_code {
        StatusErrorCode::InvalidRequest => SkuffenStatusErrorCode::InvalidRequest,
        StatusErrorCode::NotFound => SkuffenStatusErrorCode::NotFound,
        StatusErrorCode::Conflict => SkuffenStatusErrorCode::Conflict,
        StatusErrorCode::PrerequisitePending => SkuffenStatusErrorCode::PrerequisitePending,
        StatusErrorCode::TemporaryUnavailable => SkuffenStatusErrorCode::TemporaryUnavailable,
        StatusErrorCode::ProcessingFailed => SkuffenStatusErrorCode::ProcessingFailed,
    }
}

fn command_event(hendelse: CommandEvent) -> SkuffenCommandEvent {
    match hendelse {
        CommandEvent::Mottatt => SkuffenCommandEvent::Mottatt,
        CommandEvent::Validert => SkuffenCommandEvent::Validert,
        CommandEvent::Avvist => SkuffenCommandEvent::Avvist,
        CommandEvent::Utfores => SkuffenCommandEvent::Utfores,
        CommandEvent::Fullfort => SkuffenCommandEvent::Fullfort,
        CommandEvent::Feilet => SkuffenCommandEvent::Feilet,
        CommandEvent::KreverAvklaring => SkuffenCommandEvent::KreverAvklaring,
    }
}

fn operasjonshendelse(hendelse: Operasjonshendelse) -> SkuffenOperasjonHendelse {
    match hendelse {
        Operasjonshendelse::ForsokFeilet => SkuffenOperasjonHendelse::ForsokFeilet,
        Operasjonshendelse::Ok => SkuffenOperasjonHendelse::Ok,
        Operasjonshendelse::Feilet => SkuffenOperasjonHendelse::Feilet,
        Operasjonshendelse::KreverAvklaring => SkuffenOperasjonHendelse::KreverAvklaring,
        Operasjonshendelse::Varsel => SkuffenOperasjonHendelse::Varsel,
    }
}

fn operasjonstype(operasjonstype: Operasjonstype) -> SkuffenOperasjonstype {
    match operasjonstype {
        Operasjonstype::OpprettSak => SkuffenOperasjonstype::OpprettSak,
        Operasjonstype::RenderDokument => SkuffenOperasjonstype::RenderDokument,
        Operasjonstype::OpprettJournalpost => SkuffenOperasjonstype::OpprettJournalpost,
        Operasjonstype::LeggTilVedlegg => SkuffenOperasjonstype::LeggTilVedlegg,
        Operasjonstype::Journalfor => SkuffenOperasjonstype::Journalfor,
        Operasjonstype::SettEkspedert => SkuffenOperasjonstype::SettEkspedert,
        Operasjonstype::KlargjorForEkspedering => SkuffenOperasjonstype::KlargjorForEkspedering,
        Operasjonstype::AvventJournalfort => SkuffenOperasjonstype::AvventJournalfort,
        Operasjonstype::Avskriv => SkuffenOperasjonstype::Avskriv,
        Operasjonstype::SettSaksansvarlig => SkuffenOperasjonstype::SettSaksansvarlig,
        Operasjonstype::AvsluttSak => SkuffenOperasjonstype::AvsluttSak,
    }
}

fn parse_uuid(verdi: &Option<String>) -> Option<Uuid> {
    verdi.as_deref().and_then(|v| Uuid::parse_str(v).ok())
}

pub fn to_public_command_status(status: &CommandStatus) -> SkuffenCommandStatusV1 {
    let kontekst = &status.kontekst;

    SkuffenCommandStatusV1 {
        command_id: status.command_id,
        correlation_id: status.correlation_id,
        hendelse: command_event(status.hendelse),
        terminal: status.terminal,
        message: status.melding.clone(),
        error_code: status.error_code.map(error_code),
        sak_client_reference: parse_uuid(&kontekst.sak_client_reference),
        // Saksnummer valideres ved konstruksjon; en verdi som ikke passer
        // kontrakten utelates heller enn å sendes rå.
        saksnummer: kontekst
            .saksnummer
            .as_deref()
            .and_then(|verdi| Saksnummer::new(verdi).ok()),
        journalpost_client_reference: parse_uuid(&kontekst.journalpost_client_reference),
        journalpost_id: kontekst
            .journalpost_arkiv_id
            .as_ref()
            .map(|id| JournalpostId(id.clone())),
        dokument_client_references: if kontekst.dokument_client_references.is_empty() {
            None
        } else {
            Some(
                kontekst
                    .dokument_client_references
                    .iter()
                    .filter_map(|verdi| Uuid::parse_str(verdi).ok())
                    .collect(),
            )
        },
        timestamp: Some(status.timestamp.clone()),
    }
}

pub fn to_public_operasjonstatus(status: &Operasjonstatus) -> SkuffenOperasjonStatusV1 {
    SkuffenOperasjonStatusV1 {
        command_id: status.command_id,
        correlation_id: status.correlation_id,
        operasjon_id: status.operasjon_id.0,
        operasjonstype: operasjonstype(status.operasjonstype),
        hendelse: operasjonshendelse(status.hendelse),
        terminal: status.terminal,
        message: status.melding.clone(),
        error_code: status.error_code.map(error_code),
        attempt: u32::try_from(status.attempt_no).ok().filter(|n| *n > 0),
        timestamp: Some(status.timestamp.clone()),
    }
}

/// `arkiv.status.<command_id>.command`
///
/// Kommandoeventet ligger på `.command` og ikke på `arkiv.status.<cmd>` fordi
/// `arkiv.status.<cmd>.>` ikke matcher `arkiv.status.<cmd>` selv. Ett ekstra
/// token kjøper én subscription for hele historikken.
pub fn command_subject(command_id: Uuid) -> String {
    format!("arkiv.status.{command_id}.command")
}

/// `arkiv.status.<command_id>.operasjon.<operasjon_id>`
pub fn operasjon_subject(command_id: Uuid, operasjon_id: Uuid) -> String {
    format!("arkiv.status.{command_id}.operasjon.{operasjon_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::eksekvering::operasjon::OperasjonId;
    use domain::eksekvering::typer::Statuskontekst;

    #[test]
    fn subjects_folger_kontrakten() {
        let command_id = Uuid::from_u128(1);
        let operasjon_id = Uuid::from_u128(2);

        assert_eq!(
            command_subject(command_id),
            format!("arkiv.status.{command_id}.command")
        );
        assert_eq!(
            operasjon_subject(command_id, operasjon_id),
            format!("arkiv.status.{command_id}.operasjon.{operasjon_id}")
        );
    }

    #[test]
    fn begge_subjektdybder_matches_av_full_logg_wildcard() {
        // `arkiv.status.<cmd>.>` skal fange begge.
        let command_id = Uuid::from_u128(3);
        let command = command_subject(command_id);
        let operasjon = operasjon_subject(command_id, Uuid::from_u128(4));
        let prefiks = format!("arkiv.status.{command_id}.");

        assert!(command.starts_with(&prefiks));
        assert!(operasjon.starts_with(&prefiks));
    }

    #[test]
    fn command_status_mapper_kontekst() {
        let sak_ref = Uuid::from_u128(5);
        let status = CommandStatus::new(
            Uuid::from_u128(6),
            None,
            domain::eksekvering::typer::CommandTypeCode::OpprettSak,
            CommandEvent::Fullfort,
            "Forespørselen er fullført.",
            None,
            Statuskontekst {
                sak_client_reference: Some(sak_ref.to_string()),
                saksnummer: Some("2026/123".to_string()),
                ..Default::default()
            },
        );

        let wire = to_public_command_status(&status);

        assert_eq!(wire.hendelse, SkuffenCommandEvent::Fullfort);
        assert!(wire.terminal);
        assert_eq!(wire.sak_client_reference, Some(sak_ref));
        assert_eq!(wire.saksnummer, Saksnummer::new("2026/123").ok());
        assert_eq!(wire.journalpost_id, None);
    }

    #[test]
    fn attempt_null_utelates() {
        let status = Operasjonstatus::new(
            Uuid::from_u128(7),
            None,
            OperasjonId(Uuid::from_u128(8)),
            Operasjonstype::AvsluttSak,
            Operasjonshendelse::Ok,
            0,
            "Allerede utført.",
            None,
        );

        assert_eq!(to_public_operasjonstatus(&status).attempt, None);
    }

    #[test]
    fn varsel_er_ikke_terminalt_utad() {
        let status = Operasjonstatus::new(
            Uuid::from_u128(9),
            None,
            OperasjonId(Uuid::from_u128(10)),
            Operasjonstype::AvventJournalfort,
            Operasjonshendelse::Varsel,
            0,
            "Operasjonen har ikke fullført innen fristen.",
            Some(StatusErrorCode::PrerequisitePending),
        );

        let wire = to_public_operasjonstatus(&status);

        assert_eq!(wire.hendelse, SkuffenOperasjonHendelse::Varsel);
        assert!(!wire.terminal);
    }
}
