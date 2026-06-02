//! Centralized helpers for `CommandStateDecision` detail formatting and mapping.
//!
//! This module provides narrow, reusable utilities for formatting decision details
//! and mapping decisions to execution status. All callers use pub(super) visibility
//! to keep the API surface internal to the services module.

use domain::eksekvering::execution::EksekveringStatus;
use domain::eksekvering::tilstand::{BlockedReason, CommandStateDecision, DomainViolation};

/// Format a blocked reason into a safe, structured detail string.
pub(super) fn blocked_detail(reason: BlockedReason) -> String {
    format!(
        "{} trigger_category={}",
        reason.safe_detail(),
        reason.trigger_category().as_code()
    )
}

/// Format a domain violation into a safe detail string.
pub(super) fn invalid_detail(violation: DomainViolation) -> String {
    violation.safe_detail().to_string()
}

/// Map a `CommandStateDecision` to the initial queue status used by registration.
///
/// Registration only decides whether a command starts ready or blocked. Terminal
/// Done/Invalid decisions start as Klar so the executor owns Ok/Feil lifecycle
/// events and final status transitions.
pub(super) fn registration_initial_status(
    decision: CommandStateDecision,
) -> (EksekveringStatus, Option<String>) {
    match decision {
        CommandStateDecision::Ready(_)
        | CommandStateDecision::Done
        | CommandStateDecision::Invalid(_) => (EksekveringStatus::Klar, None),
        CommandStateDecision::Blocked(reason) => (
            EksekveringStatus::BlokkertVenter,
            Some(blocked_detail(reason)),
        ),
    }
}
