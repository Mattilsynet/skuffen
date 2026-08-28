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
                Some(command.felles().client_reference)
            }
            Self::AvsluttSak(_) | Self::SettSaksansvarlig(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpprettSakCommand {
    pub client_reference: Uuid,
    pub sakstittel: String,
    pub ordningsverdi: domain::model::sak::Ordningsverdi,
    pub arkivdel: Arkivdel,
    pub saksbehandler_id: String,
    pub saksbehandler_enhet: String,
    pub tilgjengelighet: Tilgjengelighet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tilgjengelighet {
    Offentlig,
    Skjermet {
        tilgangskode: domain::model::tilgang::Tilgangskode,
        tilgangshjemmel: domain::model::tilgang::Tilgangshjemmel,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arkivdel {
    Tilsynsdivisjonene,
    Hovedkontoret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parttype {
    Person,
    Virksomhet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Korrespondansepart {
    pub navn: String,
    pub parttype: Parttype,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MottakerId {
    Person {
        fødselsnummer: domain::model::identifikator::Fødselsnummer,
    },
    Virksomhet {
        organisasjonsnummer: domain::model::identifikator::Organisasjonsnummer,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Postadresse {
    pub adresse: String,
    pub postnummer: domain::model::identifikator::Postnummer,
    pub poststed: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utsendingsmottaker {
    pub navn: String,
    pub id: MottakerId,
    pub adresse: Postadresse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpprettJournalpostCommand {
    Inngaende {
        felles: JournalpostCommon,
        avsender: Korrespondansepart,
    },
    Utgaaende {
        felles: JournalpostCommon,
        mottakere: Vec<Korrespondansepart>,
    },
    UtgaaendeMedUtsending {
        felles: JournalpostCommon,
        mottakere: Vec<Utsendingsmottaker>,
    },
    InterntNotat {
        felles: JournalpostCommon,
    },
}

impl OpprettJournalpostCommand {
    pub fn felles(&self) -> &JournalpostCommon {
        match self {
            Self::Inngaende { felles, .. }
            | Self::Utgaaende { felles, .. }
            | Self::UtgaaendeMedUtsending { felles, .. }
            | Self::InterntNotat { felles } => felles,
        }
    }

    pub fn med_utsending(&self) -> bool {
        matches!(self, Self::UtgaaendeMedUtsending { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostCommon {
    pub client_reference: Uuid,
    pub tittel: String,
    pub dokument_dato: String,
    pub saksbehandler: String,
    pub saksbehandler_enhet: String,
    pub tilgjengelighet: Tilgjengelighet,
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
pub mod test_fixtures {
    use super::*;

    pub fn envelope(payload: Command) -> CommandEnvelope<Command> {
        CommandEnvelope::new(Uuid::new_v4(), payload).with_correlation_id(Uuid::new_v4())
    }

    pub fn opprett_sak(client_reference: Uuid) -> Command {
        Command::OpprettSak(OpprettSakCommand {
            client_reference,
            sakstittel: "Tilsynssak".to_string(),
            ordningsverdi: domain::model::sak::Ordningsverdi::new("123".to_string())
                .expect("syntetisk ordningsverdi er gyldig"),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: Tilgjengelighet::Offentlig,
        })
    }

    pub fn opprett_sak_envelope(
        command_id: Uuid,
        client_reference: Uuid,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope::new(command_id, opprett_sak(client_reference))
            .with_correlation_id(Uuid::new_v4())
    }

    pub fn internt_notat(sak_key: SakKey) -> Command {
        Command::OpprettInterntNotatJournalpost(OpprettJournalpostCommand::InterntNotat {
            felles: JournalpostCommon {
                client_reference: Uuid::new_v4(),
                tittel: "Internt notat".to_string(),
                dokument_dato: "2025-01-01".to_string(),
                saksbehandler: "Z12345".to_string(),
                saksbehandler_enhet: "42".to_string(),
                tilgjengelighet: Tilgjengelighet::Offentlig,
                dokumenter: vec![Dokument {
                    client_reference: Uuid::new_v4(),
                    tittel: "Hoveddokument".to_string(),
                    form: Dokumentform::Bytes {
                        dokument_referanse: Uuid::new_v4(),
                        filtype: "PDF".to_string(),
                    },
                }],
                sak_key,
                kildesystem: None,
            },
        })
    }

    pub fn avslutt_sak(sak_key: SakKey) -> Command {
        Command::AvsluttSak(AvsluttSakCommand { sak_key })
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
