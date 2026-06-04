use domain::eksekvering::html_template::TemplateFelt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEnvelope<T> {
    pub command_id: Uuid,
    pub correlation_id: Option<Uuid>,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    OpprettSak(OpprettSakCommand),
    OpprettInngaaendeJournalpost(OpprettJournalpostCommand),
    OpprettUtgaaendeJournalpost(OpprettJournalpostCommand),
    OpprettInterntNotatJournalpost(OpprettJournalpostCommand),
    AvsluttSak(AvsluttSakCommand),
    SettSaksansvarlig(SettSaksansvarligCommand),
}

impl Command {
    pub fn client_reference(&self) -> Option<Uuid> {
        match self {
            Self::OpprettSak(command) => Some(command.client_reference),
            Self::OpprettInngaaendeJournalpost(command)
            | Self::OpprettUtgaaendeJournalpost(command)
            | Self::OpprettInterntNotatJournalpost(command) => {
                Some(command.felles.client_reference)
            }
            Self::AvsluttSak(_) | Self::SettSaksansvarlig(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpprettSakCommand {
    pub client_reference: Uuid,
    pub sakstittel: String,
    pub ordningsverdi: String,
    pub arkivdel: Arkivdel,
    pub saksbehandler_id: String,
    pub saksbehandler_enhet: String,
    pub tilgang: Option<Tilgang>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tilgang {
    pub tilgangskode: String,
    pub tilgangshjemmel: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arkivdel {
    Tilsynsdivisjonene,
    Hovedkontoret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpprettJournalpostCommand {
    pub felles: JournalpostCommon,
    pub avsender: Option<String>,
    pub mottaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostCommon {
    pub client_reference: Uuid,
    pub tittel: String,
    pub dokument_dato: String,
    pub saksbehandler: String,
    pub saksbehandler_enhet: String,
    pub tilgang: Option<Tilgang>,
    pub dokumenter: Vec<Dokument>,
    pub sak_key: SakKey,
    pub kildesystem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dokument {
    pub client_reference: Uuid,
    pub tittel: String,
    pub form: Dokumentform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dokumentform {
    Bytes {
        dokument_referanse: Uuid,
        filtype: String,
    },
    HtmlTemplate {
        mal_referanse: Uuid,
        felter: Vec<TemplateFelt>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SakKey {
    ClientReference(Uuid),
    ArkivId(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvsluttSakCommand {
    pub sak_key: SakKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettSaksansvarligCommand {
    pub sak_key: SakKey,
    pub saksbehandler_id: String,
    pub saksbehandler_enhet: String,
}

impl<T> CommandEnvelope<T> {
    pub fn new(command_id: Uuid, payload: T) -> Self {
        Self {
            command_id,
            correlation_id: None,
            payload,
        }
    }

    pub fn with_correlation_id(mut self, correlation_id: Uuid) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use lib_schemas::skuffen::command::commands::{
        Command as WireCommand, CommandEnvelope as WireCommandEnvelope,
    };
    use lib_schemas::skuffen::command::journalpost as wire_journalpost;
    use lib_schemas::skuffen::command::sak as wire_sak;
    use lib_schemas::skuffen::dokument as wire_dokument;
    use lib_schemas::skuffen::query::queries as wire_query;

    pub fn map_wire_envelope(
        envelope: WireCommandEnvelope<WireCommand>,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: envelope.command_id,
            correlation_id: envelope.correlation_id,
            payload: map_wire_command(envelope.payload),
        }
    }

    fn map_wire_command(command: WireCommand) -> Command {
        match command {
            WireCommand::OpprettSak(command) => Command::OpprettSak(map_opprett_sak(command)),
            WireCommand::OpprettInngåendeJournalpost(command) => {
                Command::OpprettInngaaendeJournalpost(map_inngaende_journalpost(command))
            }
            WireCommand::OpprettUtgåendeJournalpost(command) => {
                Command::OpprettUtgaaendeJournalpost(map_utgaaende_journalpost(command))
            }
            WireCommand::OpprettInterntNotatJournalpost(command) => {
                Command::OpprettInterntNotatJournalpost(map_internt_notat_journalpost(command))
            }
            WireCommand::AvsluttSak(command) => Command::AvsluttSak(AvsluttSakCommand {
                sak_key: map_sak_key(command.sak_key),
            }),
            WireCommand::SettSaksansvarlig(command) => {
                Command::SettSaksansvarlig(SettSaksansvarligCommand {
                    sak_key: map_sak_key(command.sak_key),
                    saksbehandler_id: command.saksbehandler_id,
                    saksbehandler_enhet: command.saksbehandler_enhet,
                })
            }
        }
    }

    fn map_opprett_sak(command: wire_sak::OpprettSak) -> OpprettSakCommand {
        OpprettSakCommand {
            client_reference: command.client_reference,
            sakstittel: command.sakstittel.0,
            ordningsverdi: command.ordningsverdi.as_str().to_string(),
            arkivdel: match command.arkivdel {
                wire_sak::Arkivdel::Tilsynsdivisjonene => Arkivdel::Tilsynsdivisjonene,
                wire_sak::Arkivdel::Hovedkontoret => Arkivdel::Hovedkontoret,
            },
            saksbehandler_id: command.saksbehandler_id,
            saksbehandler_enhet: command.saksbehandler_enhet,
            tilgang: command.tilgang.map(|tilgang| Tilgang {
                tilgangskode: tilgang.tilgangskode,
                tilgangshjemmel: tilgang.tilgangshjemmel,
            }),
        }
    }

    fn map_inngaende_journalpost(
        command: wire_journalpost::OpprettInngåendeJournalpost,
    ) -> OpprettJournalpostCommand {
        OpprettJournalpostCommand {
            felles: map_journalpost_common(command.felles),
            avsender: Some(command.avsender),
            mottaker: command.mottaker,
        }
    }

    fn map_utgaaende_journalpost(
        command: wire_journalpost::OpprettUgåendeJournalpost,
    ) -> OpprettJournalpostCommand {
        OpprettJournalpostCommand {
            felles: map_journalpost_common(command.felles),
            avsender: command.avsender,
            mottaker: Some(command.mottaker),
        }
    }

    fn map_internt_notat_journalpost(
        command: wire_journalpost::OpprettInterntNotatJournalpost,
    ) -> OpprettJournalpostCommand {
        OpprettJournalpostCommand {
            felles: map_journalpost_common(command.felles),
            avsender: None,
            mottaker: None,
        }
    }

    fn map_journalpost_common(command: wire_journalpost::JournalpostCommon) -> JournalpostCommon {
        JournalpostCommon {
            client_reference: command.client_reference,
            tittel: command.tittel,
            dokument_dato: command.dokument_dato,
            saksbehandler: command.saksbehandler,
            saksbehandler_enhet: command.saksbehandler_enhet,
            tilgang: command.tilgang.map(|tilgang| Tilgang {
                tilgangskode: tilgang.tilgangskode,
                tilgangshjemmel: tilgang.tilgangshjemmel,
            }),
            dokumenter: command
                .dokumenter
                .into_iter()
                .map(|dokument| Dokument {
                    client_reference: dokument.client_reference,
                    tittel: dokument.tittel,
                    form: match dokument.form {
                        wire_dokument::Dokumentform::Bytes {
                            dokument_referanse,
                            filtype,
                        } => Dokumentform::Bytes {
                            dokument_referanse,
                            filtype,
                        },
                        wire_dokument::Dokumentform::HtmlTemplate {
                            mal_referanse,
                            felter,
                        } => Dokumentform::HtmlTemplate {
                            mal_referanse,
                            felter: felter
                                .into_iter()
                                .map(|felt| match felt {
                                    wire_dokument::Felt::Saksnummer => TemplateFelt::Saksnummer,
                                })
                                .collect(),
                        },
                    },
                })
                .collect(),
            sak_key: map_sak_key(command.sak_key),
            kildesystem: command.kildesystem,
        }
    }

    fn map_sak_key(sak_key: wire_query::SakKey) -> SakKey {
        match sak_key {
            wire_query::SakKey::ClientReference(client_reference) => {
                SakKey::ClientReference(client_reference)
            }
            wire_query::SakKey::ArkivId(saksnummer) => {
                SakKey::ArkivId(saksnummer.as_str().to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_command_id_and_payload_without_correlation_id() {
        let command_id = Uuid::new_v4();
        let envelope = CommandEnvelope::new(command_id, "payload");

        assert_eq!(envelope.command_id, command_id);
        assert_eq!(envelope.correlation_id, None);
        assert_eq!(envelope.payload, "payload");
    }

    #[test]
    fn with_correlation_id_sets_optional_metadata() {
        let correlation_id = Uuid::new_v4();
        let envelope =
            CommandEnvelope::new(Uuid::new_v4(), "payload").with_correlation_id(correlation_id);

        assert_eq!(envelope.correlation_id, Some(correlation_id));
    }
}
