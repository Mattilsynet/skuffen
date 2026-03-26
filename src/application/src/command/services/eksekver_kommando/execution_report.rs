use domain::eksekvering::typer::CommandLifecycleContext;

use super::prerequisite::Prerequisite;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionRefs {
    pub saksnummer: Option<String>,
    pub journalpost_id: Option<String>,
    pub dokument_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionReport {
    pub refs: ExecutionRefs,
    pub detail: Option<String>,
    pub blocked_by: Option<Prerequisite>,
}

impl ExecutionReport {
    pub fn into_context(self) -> CommandLifecycleContext {
        CommandLifecycleContext {
            sak_client_reference: None,
            saksnummer: self.refs.saksnummer,
            journalpost_client_reference: None,
            journalpost_id: self.refs.journalpost_id,
            dokument_client_references: Vec::new(),
            dokument_ids: self.refs.dokument_ids,
        }
    }

    pub fn merge_context_over(
        self,
        mut context: CommandLifecycleContext,
    ) -> CommandLifecycleContext {
        if let Some(saksnummer) = &self.refs.saksnummer {
            context.saksnummer = Some(saksnummer.clone());
        }

        if let Some(journalpost_id) = &self.refs.journalpost_id {
            context.journalpost_id = Some(journalpost_id.clone());
        }

        for dokument_id in &self.refs.dokument_ids {
            if !context.dokument_ids.contains(dokument_id) {
                context.dokument_ids.push(dokument_id.clone());
            }
        }

        context
    }

    pub fn set_saksnummer(&mut self, saksnummer: String) {
        self.refs.saksnummer = Some(saksnummer);
    }

    pub fn set_journalpost_id(&mut self, journalpost_id: impl Into<String>) {
        self.refs.journalpost_id = Some(journalpost_id.into());
    }

    pub fn add_dokument_id(&mut self, dokument_id: impl Into<String>) {
        self.refs.dokument_ids.push(dokument_id.into());
    }

    pub fn block(&mut self, prerequisite: Option<Prerequisite>, detail: impl Into<String>) {
        self.blocked_by = prerequisite;
        self.detail = Some(detail.into());
    }
}
