use super::prerequisite::Prerequisite;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionReport {
    pub saksnummer: Option<String>,
    pub journalpostnummer: Option<i32>,
    pub detail: Option<String>,
    pub prerequisite: Option<Prerequisite>,
}

impl ExecutionReport {
    pub fn set_saksnummer(&mut self, saksnummer: String) {
        self.saksnummer = Some(saksnummer);
    }

    pub fn set_journalpostnummer(&mut self, journalpostnummer: i32) {
        self.journalpostnummer = Some(journalpostnummer);
    }

    pub fn block(&mut self, prerequisite: Option<Prerequisite>, detail: impl Into<String>) {
        self.prerequisite = prerequisite;
        self.detail = Some(detail.into());
    }
}
