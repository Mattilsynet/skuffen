use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SkuffenSakId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SkuffenJournalpostId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SkuffenDokumentId(pub Uuid);

impl From<Uuid> for SkuffenSakId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<SkuffenSakId> for Uuid {
    fn from(value: SkuffenSakId) -> Self {
        value.0
    }
}

impl From<Uuid> for SkuffenJournalpostId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<SkuffenJournalpostId> for Uuid {
    fn from(value: SkuffenJournalpostId) -> Self {
        value.0
    }
}

impl From<Uuid> for SkuffenDokumentId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<SkuffenDokumentId> for Uuid {
    fn from(value: SkuffenDokumentId) -> Self {
        value.0
    }
}
