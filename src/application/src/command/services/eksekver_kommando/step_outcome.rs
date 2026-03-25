use super::prerequisite::Prerequisite;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Completed,
    AlreadyCompleted,
    Blocked {
        prerequisite: Option<Prerequisite>,
        detail: String,
    },
}

impl StepOutcome {
    pub fn blocked(prerequisite: Option<Prerequisite>, detail: impl Into<String>) -> Self {
        Self::Blocked {
            prerequisite,
            detail: detail.into(),
        }
    }
}
