use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, Korrespondansepart, MottakerId, OpprettInngåendeJournalpost,
    OpprettInterntNotatJournalpost, OpprettUtgåendeJournalpost,
    OpprettUtgåendeJournalpostMedUtsending, Parttype, Postadresse, Utsendingsmottaker,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak, SettSaksansvarlig};
use lib_schemas::skuffen::dokument::{Dokument as DtoDokument, Dokumentform};
use lib_schemas::skuffen::journalpost::Postnummer;
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::tilgang::{Tilgangshjemmel, Tilgangskode, Tilgjengelighet};
use lib_schemas::typer::organisasjonsnummer::Organisasjonsnummer;
use lib_schemas::typer::personnummer::Personnummer;
use uuid::Uuid;

pub struct CommandScenario {
    pub sak_client_reference: Uuid,
    #[allow(dead_code)]
    pub sak_skuffen_id: Uuid,
    pub journalpost_inngaende_client_reference: Uuid,
    pub journalpost_utgaaende_client_reference: Uuid,
    pub journalpost_internt_client_reference: Uuid,
    pub journalpost_utgaaende_utsending_client_reference: Uuid,
    pub dokument_client_reference: Uuid,
    pub dokument_referanse: Uuid,
    pub vedlegg_client_reference: Uuid,
    pub mal_referanse: Uuid,
}

impl CommandScenario {
    pub fn new() -> Self {
        Self {
            sak_client_reference: Uuid::new_v4(),
            sak_skuffen_id: Uuid::new_v4(),
            journalpost_inngaende_client_reference: Uuid::new_v4(),
            journalpost_utgaaende_client_reference: Uuid::new_v4(),
            journalpost_internt_client_reference: Uuid::new_v4(),
            journalpost_utgaaende_utsending_client_reference: Uuid::new_v4(),
            dokument_client_reference: Uuid::new_v4(),
            dokument_referanse: Uuid::new_v4(),
            vedlegg_client_reference: Uuid::new_v4(),
            mal_referanse: Uuid::new_v4(),
        }
    }

    pub fn skjermet() -> Tilgjengelighet {
        Tilgjengelighet::Skjermet {
            tilgangskode: Tilgangskode::new("UO").unwrap(),
            tilgangshjemmel: Tilgangshjemmel::new("Offl. § 13").unwrap(),
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
                    sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(sakstittel)
                        .unwrap(),
                    arkivdel: Arkivdel::Tilsynsdivisjonene,
                    saksbehandler_id: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())
                        .unwrap(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
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
                        tilgjengelighet: Tilgjengelighet::Offentlig,
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

    pub fn opprett_sak_med_tilgjengelighet(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sakstittel: String,
        tilgjengelighet: Tilgjengelighet,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettSak(OpprettSak {
                client_reference: self.sak_client_reference,
                sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(sakstittel).unwrap(),
                arkivdel: Arkivdel::Tilsynsdivisjonene,
                saksbehandler_id: saksbehandler_id.to_string(),
                saksbehandler_enhet: saksbehandler_enhet.to_string(),
                ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())
                    .unwrap(),
                tilgjengelighet,
            }),
        }
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
                    tilgjengelighet: Tilgjengelighet::Offentlig,
                    dokumenter: vec![DtoDokument {
                        client_reference: self.dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        form: self.bytes_form(),
                    }],
                    sak_key,
                    kildesystem: None,
                },
                avsender: Korrespondansepart {
                    navn: "Avsender".to_string(),
                    parttype: Parttype::Virksomhet,
                },
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
            payload: Command::OpprettUtgåendeJournalpost(OpprettUtgåendeJournalpost {
                felles: JournalpostCommon {
                    client_reference: self.journalpost_utgaaende_client_reference,
                    tittel: title.to_string(),
                    dokument_dato: "2025-01-03".to_string(),
                    saksbehandler: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
                    dokumenter: vec![DtoDokument {
                        client_reference: self.dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        form: self.bytes_form(),
                    }],
                    sak_key,
                    kildesystem: None,
                },
                mottakere: vec![Korrespondansepart {
                    navn: "Mottaker".to_string(),
                    parttype: Parttype::Virksomhet,
                }],
            }),
        }
    }

    pub fn opprett_skjermet_internt_notat(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sak_key: DtoSakKey,
        title: &str,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                felles: JournalpostCommon {
                    client_reference: self.journalpost_internt_client_reference,
                    tittel: title.to_string(),
                    dokument_dato: "2025-01-04".to_string(),
                    saksbehandler: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    tilgjengelighet: Self::skjermet(),
                    dokumenter: vec![DtoDokument {
                        client_reference: self.dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        form: self.bytes_form(),
                    }],
                    sak_key,
                    kildesystem: None,
                },
            }),
        }
    }

    pub fn opprett_internt_notat_med_ugyldig_markup(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sak_key: DtoSakKey,
        title: &str,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                felles: JournalpostCommon {
                    client_reference: self.journalpost_internt_client_reference,
                    tittel: title.to_string(),
                    dokument_dato: "2025-01-04".to_string(),
                    saksbehandler: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
                    dokumenter: vec![DtoDokument {
                        client_reference: self.dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        form: self.bytes_form(),
                    }],
                    sak_key,
                    kildesystem: None,
                },
            }),
        }
    }

    /// Internt notat der vedlegget er en HTML-mal. Domenet avviser det
    /// (`HtmlTemplateVedleggIkkeStottet`) ved første vurdering, uten å ha
    /// vært innom arkivet.
    pub fn opprett_internt_notat_med_html_vedlegg(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sak_key: DtoSakKey,
        title: &str,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                felles: JournalpostCommon {
                    client_reference: self.journalpost_internt_client_reference,
                    tittel: title.to_string(),
                    dokument_dato: "2025-01-05".to_string(),
                    saksbehandler: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
                    dokumenter: vec![
                        DtoDokument {
                            client_reference: self.dokument_client_reference,
                            tittel: "Hoveddokument".to_string(),
                            form: self.bytes_form(),
                        },
                        DtoDokument {
                            client_reference: self.vedlegg_client_reference,
                            tittel: "Vedlegg som mal".to_string(),
                            form: Dokumentform::HtmlTemplate {
                                mal_referanse: self.mal_referanse,
                                felter: Vec::new(),
                            },
                        },
                    ],
                    sak_key,
                    kildesystem: None,
                },
            }),
        }
    }

    pub fn opprett_utgaaende_flere_mottakere(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sak_key: DtoSakKey,
        title: &str,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettUtgåendeJournalpost(OpprettUtgåendeJournalpost {
                felles: JournalpostCommon {
                    client_reference: self.journalpost_utgaaende_client_reference,
                    tittel: title.to_string(),
                    dokument_dato: "2025-01-03".to_string(),
                    saksbehandler: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
                    dokumenter: vec![DtoDokument {
                        client_reference: self.dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        form: self.bytes_form(),
                    }],
                    sak_key,
                    kildesystem: None,
                },
                mottakere: vec![
                    Korrespondansepart {
                        navn: "Mottaker En".to_string(),
                        parttype: Parttype::Virksomhet,
                    },
                    Korrespondansepart {
                        navn: "Mottaker To".to_string(),
                        parttype: Parttype::Person,
                    },
                ],
            }),
        }
    }

    pub fn opprett_utgaaende_med_utsending(
        &self,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
        sak_key: DtoSakKey,
        title: &str,
    ) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettUtgåendeJournalpostMedUtsending(
                OpprettUtgåendeJournalpostMedUtsending {
                    felles: JournalpostCommon {
                        client_reference: self.journalpost_utgaaende_utsending_client_reference,
                        tittel: title.to_string(),
                        dokument_dato: "2025-01-03".to_string(),
                        saksbehandler: saksbehandler_id.to_string(),
                        saksbehandler_enhet: saksbehandler_enhet.to_string(),
                        tilgjengelighet: Tilgjengelighet::Offentlig,
                        dokumenter: vec![DtoDokument {
                            client_reference: self.dokument_client_reference,
                            tittel: "Vedlegg".to_string(),
                            form: self.bytes_form(),
                        }],
                        sak_key,
                        kildesystem: None,
                    },
                    mottakere: vec![Utsendingsmottaker {
                        navn: "Ola Nordmann".to_string(),
                        id: MottakerId::Person {
                            fødselsnummer: Personnummer::new("01010101006").unwrap(),
                        },
                        adresse: Postadresse {
                            adresse: "Storgata 1".to_string(),
                            postnummer: Postnummer::new("0350").unwrap(),
                            poststed: "Oslo".to_string(),
                        },
                    }],
                },
            ),
        }
    }

    #[allow(dead_code)]
    pub fn virksomhet_mottaker() -> Utsendingsmottaker {
        Utsendingsmottaker {
            navn: "Bedrift AS".to_string(),
            id: MottakerId::Virksomhet {
                organisasjonsnummer: Organisasjonsnummer::new("995298775").unwrap(),
            },
            adresse: Postadresse {
                adresse: "Storgata 2".to_string(),
                postnummer: Postnummer::new("0350").unwrap(),
                poststed: "Oslo".to_string(),
            },
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
