use crate::command::services::eksekver_kommando::IntoExecutorEnvelope;
use crate::command::services::ingest_command::IntoCommandBatch;
use crate::command::services::registrer_i_eksekveringssystem::IntoRegistrationEnvelope;
use crate::command::services::validate_command::IntoCommandEnvelope;
use crate::command::{
    Command as ApplicationCommand, CommandEnvelope as ApplicationCommandEnvelope, test_support,
};
use lib_schemas::skuffen::command::commands::{
    Command as WireCommand, CommandEnvelope as WireCommandEnvelope, CommandSequence,
};

impl IntoCommandBatch for CommandSequence {
    fn into_command_batch(self) -> Vec<ApplicationCommandEnvelope<ApplicationCommand>> {
        self.into_iter()
            .map(test_support::map_wire_envelope)
            .collect()
    }
}

impl IntoCommandEnvelope for WireCommandEnvelope<WireCommand> {
    fn into_command_envelope(self) -> ApplicationCommandEnvelope<ApplicationCommand> {
        test_support::map_wire_envelope(self)
    }
}

impl IntoExecutorEnvelope for WireCommandEnvelope<WireCommand> {
    fn into_executor_envelope(self) -> ApplicationCommandEnvelope<ApplicationCommand> {
        test_support::map_wire_envelope(self)
    }
}

impl IntoRegistrationEnvelope for &WireCommandEnvelope<WireCommand> {
    fn into_registration_envelope(self) -> ApplicationCommandEnvelope<ApplicationCommand> {
        test_support::map_wire_envelope(self.clone())
    }
}
