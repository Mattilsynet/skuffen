use crate::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};
use crate::eksekvering::typer::CommandTypeCode;

/// Narrow domain command for planning archive execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    OpprettSak {
        sak_id: SkuffenSakId,
    },
    OpprettInngaaendeJournalpost {
        sak_id: SkuffenSakId,
        journalpost_id: SkuffenJournalpostId,
    },
    OpprettUtgaaendeJournalpost {
        sak_id: SkuffenSakId,
        journalpost_id: SkuffenJournalpostId,
    },
    OpprettInterntNotatJournalpost {
        sak_id: SkuffenSakId,
        journalpost_id: SkuffenJournalpostId,
    },
    AvsluttSak {
        sak_id: SkuffenSakId,
    },
    SettSaksansvarlig {
        sak_id: SkuffenSakId,
    },
}

impl Command {
    pub fn command_type(&self) -> CommandTypeCode {
        match self {
            Self::OpprettSak { .. } => CommandTypeCode::OpprettSak,
            Self::OpprettInngaaendeJournalpost { .. } => {
                CommandTypeCode::OpprettInngaaendeJournalpost
            }
            Self::OpprettUtgaaendeJournalpost { .. } => {
                CommandTypeCode::OpprettUtgaaendeJournalpost
            }
            Self::OpprettInterntNotatJournalpost { .. } => {
                CommandTypeCode::OpprettInterntNotatJournalpost
            }
            Self::AvsluttSak { .. } => CommandTypeCode::AvsluttSak,
            Self::SettSaksansvarlig { .. } => CommandTypeCode::SettSaksansvarlig,
        }
    }

    pub fn sak_id(&self) -> SkuffenSakId {
        match self {
            Self::OpprettSak { sak_id }
            | Self::OpprettInngaaendeJournalpost { sak_id, .. }
            | Self::OpprettUtgaaendeJournalpost { sak_id, .. }
            | Self::OpprettInterntNotatJournalpost { sak_id, .. }
            | Self::AvsluttSak { sak_id }
            | Self::SettSaksansvarlig { sak_id } => *sak_id,
        }
    }

    pub fn journalpost_id(&self) -> Option<SkuffenJournalpostId> {
        match self {
            Self::OpprettInngaaendeJournalpost { journalpost_id, .. }
            | Self::OpprettUtgaaendeJournalpost { journalpost_id, .. }
            | Self::OpprettInterntNotatJournalpost { journalpost_id, .. } => Some(*journalpost_id),
            Self::OpprettSak { .. } | Self::AvsluttSak { .. } | Self::SettSaksansvarlig { .. } => {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn sak_id() -> SkuffenSakId {
        SkuffenSakId(Uuid::new_v4())
    }

    fn journalpost_id() -> SkuffenJournalpostId {
        SkuffenJournalpostId(Uuid::new_v4())
    }

    #[test]
    fn sak_command_har_type_og_sak_id() {
        let sak_id = sak_id();
        let command = Command::OpprettSak { sak_id };

        assert_eq!(command.command_type(), CommandTypeCode::OpprettSak);
        assert_eq!(command.sak_id(), sak_id);
        assert_eq!(command.journalpost_id(), None);
    }

    #[test]
    fn journalpost_command_har_type_sak_id_og_journalpost_id() {
        let sak_id = sak_id();
        let journalpost_id = journalpost_id();
        let command = Command::OpprettUtgaaendeJournalpost {
            sak_id,
            journalpost_id,
        };

        assert_eq!(
            command.command_type(),
            CommandTypeCode::OpprettUtgaaendeJournalpost
        );
        assert_eq!(command.sak_id(), sak_id);
        assert_eq!(command.journalpost_id(), Some(journalpost_id));
    }
}
