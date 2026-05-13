use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInngåendeJournalpost, OpprettInterntNotatJournalpost,
    OpprettUgåendeJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak, SettSaksansvarlig};
use lib_schemas::skuffen::dokument::{Dokument as DtoDokument, Dokumentform};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use uuid::Uuid;

pub struct CommandScenario {
    pub sak_client_reference: Uuid,
    #[allow(dead_code)]
    pub sak_skuffen_id: Uuid,
    pub journalpost_inngaende_client_reference: Uuid,
    pub journalpost_utgaaende_client_reference: Uuid,
    pub journalpost_internt_client_reference: Uuid,
    pub dokument_client_reference: Uuid,
    pub dokument_referanse: Uuid,
}

impl CommandScenario {
    pub fn new() -> Self {
        Self {
            sak_client_reference: Uuid::new_v4(),
            sak_skuffen_id: Uuid::new_v4(),
            journalpost_inngaende_client_reference: Uuid::new_v4(),
            journalpost_utgaaende_client_reference: Uuid::new_v4(),
            journalpost_internt_client_reference: Uuid::new_v4(),
            dokument_client_reference: Uuid::new_v4(),
            dokument_referanse: Uuid::new_v4(),
        }
    }

    pub fn build_sequence(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sakstittel: String,
        journalpost_tittel: String,
    ) -> Vec<CommandEnvelope<Command>> {
        vec![
            CommandEnvelope {
                command_id: Uuid::new_v4(),
                correlation_id: Some(Uuid::new_v4()),
                payload: Command::OpprettSak(OpprettSak {
                    client_reference: self.sak_client_reference,
                    sakstittel: lib_schemas::skuffen::sak::Sakstittel(sakstittel),
                    arkivdel: Arkivdel::Tilsynsdivisjonene,
                    saksbehandler_id: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())
                        .unwrap(),
                    tilgang: None,
                }),
            },
            CommandEnvelope {
                command_id: Uuid::new_v4(),
                correlation_id: Some(Uuid::new_v4()),
                payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                    felles: JournalpostCommon {
                        client_reference: self.journalpost_internt_client_reference,
                        tittel: journalpost_tittel,
                        dokument_dato: "2025-01-01".to_string(),
                        saksbehandler: saksbehandler_id.to_string(),
                        saksbehandler_enhet: saksbehandler_enhet.to_string(),
                        tilgang: None,
                        dokumenter: vec![DtoDokument {
                            client_reference: self.dokument_client_reference,
                            tittel: "Vedlegg".to_string(),
                            form: self.bytes_form(),
                        }],
                        sak_key: DtoSakKey::ClientReference(self.sak_client_reference),
                        kildesystem: None,
                    },
                }),
            },
            CommandEnvelope {
                command_id: Uuid::new_v4(),
                correlation_id: Some(Uuid::new_v4()),
                payload: Command::AvsluttSak(AvsluttSak {
                    sak_key: DtoSakKey::ClientReference(self.sak_client_reference),
                }),
            },
        ]
    }

    pub fn opprett_inngaende(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sak_key: DtoSakKey,
        title: &str,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
                felles: JournalpostCommon {
                    client_reference: self.journalpost_inngaende_client_reference,
                    tittel: title.to_string(),
                    dokument_dato: "2025-01-02".to_string(),
                    saksbehandler: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    tilgang: None,
                    dokumenter: vec![DtoDokument {
                        client_reference: self.dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        form: self.bytes_form(),
                    }],
                    sak_key,
                    kildesystem: None,
                },
                avsender: "Avsender".to_string(),
                mottaker: None,
            }),
        }
    }

    pub fn opprett_utgaaende(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sak_key: DtoSakKey,
        title: &str,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettUtgåendeJournalpost(OpprettUgåendeJournalpost {
                felles: JournalpostCommon {
                    client_reference: self.journalpost_utgaaende_client_reference,
                    tittel: title.to_string(),
                    dokument_dato: "2025-01-03".to_string(),
                    saksbehandler: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    tilgang: None,
                    dokumenter: vec![DtoDokument {
                        client_reference: self.dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        form: self.bytes_form(),
                    }],
                    sak_key,
                    kildesystem: None,
                },
                avsender: Some("Avsender".to_string()),
                mottaker: "Mottaker".to_string(),
            }),
        }
    }

    pub fn sett_saksansvarlig(
        &self,
        sak_key: DtoSakKey,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::SettSaksansvarlig(SettSaksansvarlig {
                sak_key,
                saksbehandler_id: saksbehandler_id.to_string(),
                saksbehandler_enhet: saksbehandler_enhet.to_string(),
            }),
        }
    }

    fn bytes_form(&self) -> Dokumentform {
        Dokumentform::Bytes {
            dokument_referanse: self.dokument_referanse,
            filtype: "PDF".to_string(),
        }
    }
}
