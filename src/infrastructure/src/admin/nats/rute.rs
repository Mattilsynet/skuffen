//! Admin-flatens rutetabell: ett eksakt subject og én stabil queue group per
//! handling (SKU-0018 R1, SKU-0023 R1). Ingen wildcard-subscription; NATS-tilgang
//! gis per subject.

pub const ADMIN_READ_COMMAND_HENT_SUBJECT: &str = "arkiv.admin.read.command.hent";
pub const ADMIN_READ_SAK_HENT_SUBJECT: &str = "arkiv.admin.read.sak.hent";
pub const ADMIN_WRITE_RETRY_COMMANDS_SUBJECT: &str = "arkiv.admin.write.retry.commands";
pub const ADMIN_WRITE_RETRY_SAK_SUBJECT: &str = "arkiv.admin.write.retry.sak";
pub const ADMIN_WRITE_FORKAST_COMMANDS_SUBJECT: &str = "arkiv.admin.write.forkast.commands";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminRute {
    HentCommand,
    HentSak,
    RetryCommands,
    RetrySak,
    ForkastCommands,
}

impl AdminRute {
    pub const ALLE: [Self; 5] = [
        Self::HentCommand,
        Self::HentSak,
        Self::RetryCommands,
        Self::RetrySak,
        Self::ForkastCommands,
    ];

    pub fn subject(self) -> &'static str {
        match self {
            Self::HentCommand => ADMIN_READ_COMMAND_HENT_SUBJECT,
            Self::HentSak => ADMIN_READ_SAK_HENT_SUBJECT,
            Self::RetryCommands => ADMIN_WRITE_RETRY_COMMANDS_SUBJECT,
            Self::RetrySak => ADMIN_WRITE_RETRY_SAK_SUBJECT,
            Self::ForkastCommands => ADMIN_WRITE_FORKAST_COMMANDS_SUBJECT,
        }
    }

    /// Navnene er stabile, så bare én instans svarer under deploy-overlapp.
    /// Et nytt navn ville latt gammel og ny instans utføre samme skriving.
    pub fn queue_group(self) -> &'static str {
        match self {
            Self::HentCommand => "skuffen-admin-read-command-hent-v1",
            Self::HentSak => "skuffen-admin-read-sak-hent-v1",
            Self::RetryCommands => "skuffen-admin-write-retry-commands-v1",
            Self::RetrySak => "skuffen-admin-write-retry-sak-v1",
            Self::ForkastCommands => "skuffen-admin-write-forkast-commands-v1",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::HentCommand => "read.command.hent",
            Self::HentSak => "read.sak.hent",
            Self::RetryCommands => "write.retry.commands",
            Self::RetrySak => "write.retry.sak",
            Self::ForkastCommands => "write.forkast.commands",
        }
    }

    pub fn er_skriving(self) -> bool {
        match self {
            Self::HentCommand | Self::HentSak => false,
            Self::RetryCommands | Self::RetrySak | Self::ForkastCommands => true,
        }
    }
}
