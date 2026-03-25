use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prerequisite {
    SakOpprettet { sak_id: Uuid },
    SaksnummerTildelt { sak_id: Uuid },
    JournalpostOpprettet { journalpost_id: Uuid },
    JournalpostnummerTildelt { journalpost_id: Uuid },
    JournalpostJournalfoert { journalpost_id: Uuid },
}
