use application::command::{
    Arkivdel, AvsluttSakCommand, Command as ApplicationCommand, CommandEnvelope, Dokument,
    Dokumentform, JournalpostCommon, OpprettJournalpostCommand, OpprettSakCommand, SakKey,
    SettSaksansvarligCommand, Tilgang,
};
use domain::eksekvering::html_template::TemplateFelt;
use lib_schemas::skuffen::command::commands::{
    Command as WireCommand, CommandEnvelope as WireCommandEnvelope,
};
use lib_schemas::skuffen::command::journalpost as wire_journalpost;
use lib_schemas::skuffen::command::sak as wire_sak;
use lib_schemas::skuffen::dokument as wire_dokument;
use lib_schemas::skuffen::query::queries as wire_query;

pub fn map_wire_envelope(
    envelope: WireCommandEnvelope<WireCommand>,
) -> CommandEnvelope<ApplicationCommand> {
    CommandEnvelope {
        command_id: envelope.command_id,
        correlation_id: envelope.correlation_id,
        payload: map_wire_command(envelope.payload),
    }
}

pub fn map_application_envelope_to_wire(
    envelope: &CommandEnvelope<ApplicationCommand>,
) -> WireCommandEnvelope<WireCommand> {
    WireCommandEnvelope {
        command_id: envelope.command_id,
        correlation_id: envelope.correlation_id,
        payload: map_application_command_to_wire(&envelope.payload),
    }
}

fn map_application_command_to_wire(command: &ApplicationCommand) -> WireCommand {
    match command {
        ApplicationCommand::OpprettSak(command) => WireCommand::OpprettSak(wire_sak::OpprettSak {
            client_reference: command.client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel(command.sakstittel.clone()),
            ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new(
                command.ordningsverdi.clone(),
            )
            .expect("internal ordningsverdi originated from valid wire command"),
            arkivdel: map_application_arkivdel(command.arkivdel),
            saksbehandler_id: command.saksbehandler_id.clone(),
            saksbehandler_enhet: command.saksbehandler_enhet.clone(),
            tilgang: command.tilgang.clone().map(map_application_tilgang),
        }),
        ApplicationCommand::OpprettInngaaendeJournalpost(command) => {
            WireCommand::OpprettInngåendeJournalpost(
                wire_journalpost::OpprettInngåendeJournalpost {
                    felles: map_application_journalpost_common(&command.felles),
                    avsender: command.avsender.clone().unwrap_or_default(),
                    mottaker: command.mottaker.clone(),
                },
            )
        }
        ApplicationCommand::OpprettUtgaaendeJournalpost(command) => {
            WireCommand::OpprettUtgåendeJournalpost(wire_journalpost::OpprettUgåendeJournalpost {
                felles: map_application_journalpost_common(&command.felles),
                avsender: command.avsender.clone(),
                mottaker: command.mottaker.clone().unwrap_or_default(),
            })
        }
        ApplicationCommand::OpprettInterntNotatJournalpost(command) => {
            WireCommand::OpprettInterntNotatJournalpost(
                wire_journalpost::OpprettInterntNotatJournalpost {
                    felles: map_application_journalpost_common(&command.felles),
                },
            )
        }
        ApplicationCommand::AvsluttSak(command) => WireCommand::AvsluttSak(wire_sak::AvsluttSak {
            sak_key: map_application_sak_key(&command.sak_key),
        }),
        ApplicationCommand::SettSaksansvarlig(command) => {
            WireCommand::SettSaksansvarlig(wire_sak::SettSaksansvarlig {
                sak_key: map_application_sak_key(&command.sak_key),
                saksbehandler_id: command.saksbehandler_id.clone(),
                saksbehandler_enhet: command.saksbehandler_enhet.clone(),
            })
        }
    }
}

fn map_application_journalpost_common(
    command: &JournalpostCommon,
) -> wire_journalpost::JournalpostCommon {
    wire_journalpost::JournalpostCommon {
        client_reference: command.client_reference,
        tittel: command.tittel.clone(),
        dokument_dato: command.dokument_dato.clone(),
        saksbehandler: command.saksbehandler.clone(),
        saksbehandler_enhet: command.saksbehandler_enhet.clone(),
        tilgang: command.tilgang.clone().map(map_application_tilgang),
        dokumenter: command
            .dokumenter
            .iter()
            .map(map_application_dokument)
            .collect(),
        sak_key: map_application_sak_key(&command.sak_key),
        kildesystem: command.kildesystem.clone(),
    }
}

fn map_application_dokument(command: &Dokument) -> wire_dokument::Dokument {
    wire_dokument::Dokument {
        client_reference: command.client_reference,
        tittel: command.tittel.clone(),
        form: map_application_dokumentform(&command.form),
    }
}

fn map_application_dokumentform(form: &Dokumentform) -> wire_dokument::Dokumentform {
    match form {
        Dokumentform::Bytes {
            dokument_referanse,
            filtype,
        } => wire_dokument::Dokumentform::Bytes {
            dokument_referanse: *dokument_referanse,
            filtype: filtype.clone(),
        },
        Dokumentform::HtmlTemplate {
            mal_referanse,
            felter,
        } => wire_dokument::Dokumentform::HtmlTemplate {
            mal_referanse: *mal_referanse,
            felter: felter.iter().map(map_application_template_felt).collect(),
        },
    }
}

fn map_application_template_felt(felt: &TemplateFelt) -> wire_dokument::Felt {
    match felt {
        TemplateFelt::Saksnummer => wire_dokument::Felt::Saksnummer,
    }
}

fn map_application_tilgang(tilgang: Tilgang) -> lib_schemas::skuffen::tilgang::Tilgang {
    lib_schemas::skuffen::tilgang::Tilgang {
        tilgangskode: tilgang.tilgangskode,
        tilgangshjemmel: tilgang.tilgangshjemmel,
    }
}

fn map_application_sak_key(sak_key: &SakKey) -> wire_query::SakKey {
    match sak_key {
        SakKey::ClientReference(client_reference) => {
            wire_query::SakKey::ClientReference(*client_reference)
        }
        SakKey::ArkivId(saksnummer) => wire_query::SakKey::ArkivId(
            lib_schemas::skuffen::sak::Saksnummer::new(saksnummer.clone())
                .expect("internal saksnummer originated from valid wire command"),
        ),
    }
}

fn map_application_arkivdel(arkivdel: Arkivdel) -> wire_sak::Arkivdel {
    match arkivdel {
        Arkivdel::Tilsynsdivisjonene => wire_sak::Arkivdel::Tilsynsdivisjonene,
        Arkivdel::Hovedkontoret => wire_sak::Arkivdel::Hovedkontoret,
    }
}

fn map_wire_command(command: WireCommand) -> ApplicationCommand {
    match command {
        WireCommand::OpprettSak(command) => {
            ApplicationCommand::OpprettSak(map_opprett_sak(command))
        }
        WireCommand::OpprettInngåendeJournalpost(command) => {
            ApplicationCommand::OpprettInngaaendeJournalpost(map_inngaende_journalpost(command))
        }
        WireCommand::OpprettUtgåendeJournalpost(command) => {
            ApplicationCommand::OpprettUtgaaendeJournalpost(map_utgaaende_journalpost(command))
        }
        WireCommand::OpprettInterntNotatJournalpost(command) => {
            ApplicationCommand::OpprettInterntNotatJournalpost(map_internt_notat_journalpost(
                command,
            ))
        }
        WireCommand::AvsluttSak(command) => ApplicationCommand::AvsluttSak(AvsluttSakCommand {
            sak_key: map_sak_key(command.sak_key),
        }),
        WireCommand::SettSaksansvarlig(command) => {
            ApplicationCommand::SettSaksansvarlig(SettSaksansvarligCommand {
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
        arkivdel: map_arkivdel(command.arkivdel),
        saksbehandler_id: command.saksbehandler_id,
        saksbehandler_enhet: command.saksbehandler_enhet,
        tilgang: command.tilgang.map(map_tilgang),
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
        tilgang: command.tilgang.map(map_tilgang),
        dokumenter: command.dokumenter.into_iter().map(map_dokument).collect(),
        sak_key: map_sak_key(command.sak_key),
        kildesystem: command.kildesystem,
    }
}

fn map_dokument(command: wire_dokument::Dokument) -> Dokument {
    Dokument {
        client_reference: command.client_reference,
        tittel: command.tittel,
        form: map_dokumentform(command.form),
    }
}

fn map_tilgang(tilgang: lib_schemas::skuffen::tilgang::Tilgang) -> Tilgang {
    Tilgang {
        tilgangskode: tilgang.tilgangskode,
        tilgangshjemmel: tilgang.tilgangshjemmel,
    }
}

fn map_dokumentform(form: wire_dokument::Dokumentform) -> Dokumentform {
    match form {
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
            felter: felter.into_iter().map(map_template_felt).collect(),
        },
    }
}

fn map_template_felt(felt: wire_dokument::Felt) -> TemplateFelt {
    match felt {
        wire_dokument::Felt::Saksnummer => TemplateFelt::Saksnummer,
    }
}

fn map_sak_key(sak_key: wire_query::SakKey) -> SakKey {
    match sak_key {
        wire_query::SakKey::ClientReference(client_reference) => {
            SakKey::ClientReference(client_reference)
        }
        wire_query::SakKey::ArkivId(saksnummer) => SakKey::ArkivId(saksnummer.as_str().to_string()),
    }
}

fn map_arkivdel(arkivdel: wire_sak::Arkivdel) -> Arkivdel {
    match arkivdel {
        wire_sak::Arkivdel::Tilsynsdivisjonene => Arkivdel::Tilsynsdivisjonene,
        wire_sak::Arkivdel::Hovedkontoret => Arkivdel::Hovedkontoret,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lib_schemas::skuffen::command::commands::Command as WireCommand;
    use lib_schemas::skuffen::command::journalpost::{
        JournalpostCommon as WireJournalpostCommon, OpprettInngåendeJournalpost,
        OpprettInterntNotatJournalpost, OpprettUgåendeJournalpost,
    };
    use lib_schemas::skuffen::command::sak::{
        Arkivdel as WireArkivdel, AvsluttSak, OpprettSak, SettSaksansvarlig,
    };
    use lib_schemas::skuffen::dokument::{
        Dokument as WireDokument, Dokumentform as WireDokumentform, Felt,
    };
    use lib_schemas::skuffen::query::queries::SakKey as WireSakKey;
    use lib_schemas::skuffen::sak::{Ordningsverdi, Saksnummer, Sakstittel};
    use lib_schemas::skuffen::tilgang::Tilgang as WireTilgang;
    use uuid::Uuid;

    #[test]
    fn maps_opprett_sak_preserving_metadata_and_sak_fields() {
        let client_reference = Uuid::new_v4();
        let envelope = wire_envelope(WireCommand::OpprettSak(OpprettSak {
            client_reference,
            sakstittel: Sakstittel("Test sak".to_string()),
            ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
            arkivdel: WireArkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: Some(wire_tilgang()),
        }));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        match mapped.payload {
            ApplicationCommand::OpprettSak(command) => {
                assert_eq!(command.client_reference, client_reference);
                assert_eq!(command.sakstittel, "Test sak");
                assert_eq!(command.ordningsverdi, "123");
                assert_eq!(command.arkivdel, Arkivdel::Tilsynsdivisjonene);
                assert_eq!(command.saksbehandler_id, "Z12345");
                assert_eq!(command.saksbehandler_enhet, "42");
                assert_eq!(
                    command.tilgang,
                    Some(Tilgang {
                        tilgangskode: "UO".to_string(),
                        tilgangshjemmel: "offl".to_string()
                    })
                );
            }
            other => panic!("expected OpprettSak, got {other:?}"),
        }
    }

    #[test]
    fn maps_inngaende_journalpost_preserving_refs_and_template_fields() {
        let journalpost_ref = Uuid::new_v4();
        let sak_ref = Uuid::new_v4();
        let dokument_ref = Uuid::new_v4();
        let mal_ref = Uuid::new_v4();
        let envelope = wire_envelope(WireCommand::OpprettInngåendeJournalpost(
            OpprettInngåendeJournalpost {
                felles: journalpost_common(
                    journalpost_ref,
                    WireSakKey::ClientReference(sak_ref),
                    WireDokumentform::HtmlTemplate {
                        mal_referanse: mal_ref,
                        felter: vec![Felt::Saksnummer],
                    },
                    dokument_ref,
                ),
                avsender: "Avsender".to_string(),
                mottaker: Some("Mottaker".to_string()),
            },
        ));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        match mapped.payload {
            ApplicationCommand::OpprettInngaaendeJournalpost(command) => {
                assert_journalpost_common(
                    &command.felles,
                    journalpost_ref,
                    SakKey::ClientReference(sak_ref),
                    dokument_ref,
                );
                assert_eq!(command.avsender.as_deref(), Some("Avsender"));
                assert_eq!(command.mottaker.as_deref(), Some("Mottaker"));
                assert_template_document(&command.felles.dokumenter[0], mal_ref);
            }
            other => panic!("expected OpprettInngaaendeJournalpost, got {other:?}"),
        }
    }

    #[test]
    fn maps_utgaaende_journalpost_preserving_refs_and_template_fields() {
        let journalpost_ref = Uuid::new_v4();
        let dokument_ref = Uuid::new_v4();
        let mal_ref = Uuid::new_v4();
        let saksnummer = Saksnummer::new("2025/42").unwrap();
        let envelope = wire_envelope(WireCommand::OpprettUtgåendeJournalpost(
            OpprettUgåendeJournalpost {
                felles: journalpost_common(
                    journalpost_ref,
                    WireSakKey::ArkivId(saksnummer),
                    WireDokumentform::HtmlTemplate {
                        mal_referanse: mal_ref,
                        felter: vec![Felt::Saksnummer],
                    },
                    dokument_ref,
                ),
                avsender: Some("Avsender".to_string()),
                mottaker: "Mottaker".to_string(),
            },
        ));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        match mapped.payload {
            ApplicationCommand::OpprettUtgaaendeJournalpost(command) => {
                assert_journalpost_common(
                    &command.felles,
                    journalpost_ref,
                    SakKey::ArkivId("2025/42".to_string()),
                    dokument_ref,
                );
                assert_eq!(command.avsender.as_deref(), Some("Avsender"));
                assert_eq!(command.mottaker.as_deref(), Some("Mottaker"));
                assert_template_document(&command.felles.dokumenter[0], mal_ref);
            }
            other => panic!("expected OpprettUtgaaendeJournalpost, got {other:?}"),
        }
    }

    #[test]
    fn maps_internt_notat_journalpost_preserving_refs_and_bytes_document() {
        let journalpost_ref = Uuid::new_v4();
        let sak_ref = Uuid::new_v4();
        let dokument_ref = Uuid::new_v4();
        let bytes_ref = Uuid::new_v4();
        let envelope = wire_envelope(WireCommand::OpprettInterntNotatJournalpost(
            OpprettInterntNotatJournalpost {
                felles: journalpost_common(
                    journalpost_ref,
                    WireSakKey::ClientReference(sak_ref),
                    WireDokumentform::Bytes {
                        dokument_referanse: bytes_ref,
                        filtype: "PDF".to_string(),
                    },
                    dokument_ref,
                ),
            },
        ));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        match mapped.payload {
            ApplicationCommand::OpprettInterntNotatJournalpost(command) => {
                assert_journalpost_common(
                    &command.felles,
                    journalpost_ref,
                    SakKey::ClientReference(sak_ref),
                    dokument_ref,
                );
                assert_eq!(command.avsender, None);
                assert_eq!(command.mottaker, None);
                match &command.felles.dokumenter[0].form {
                    Dokumentform::Bytes {
                        dokument_referanse,
                        filtype,
                    } => {
                        assert_eq!(*dokument_referanse, bytes_ref);
                        assert_eq!(filtype, "PDF");
                    }
                    other => panic!("expected bytes document, got {other:?}"),
                }
            }
            other => panic!("expected OpprettInterntNotatJournalpost, got {other:?}"),
        }
    }

    #[test]
    fn maps_avslutt_sak_preserving_sak_ref_and_metadata() {
        let sak_ref = Uuid::new_v4();
        let envelope = wire_envelope(WireCommand::AvsluttSak(AvsluttSak {
            sak_key: WireSakKey::ClientReference(sak_ref),
        }));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        assert_eq!(
            mapped.payload,
            ApplicationCommand::AvsluttSak(AvsluttSakCommand {
                sak_key: SakKey::ClientReference(sak_ref),
            })
        );
    }

    #[test]
    fn maps_sett_saksansvarlig_preserving_sak_ref_and_metadata() {
        let saksnummer = Saksnummer::new("2025/99").unwrap();
        let envelope = wire_envelope(WireCommand::SettSaksansvarlig(SettSaksansvarlig {
            sak_key: WireSakKey::ArkivId(saksnummer),
            saksbehandler_id: "Z99999".to_string(),
            saksbehandler_enhet: "43".to_string(),
        }));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        assert_eq!(
            mapped.payload,
            ApplicationCommand::SettSaksansvarlig(SettSaksansvarligCommand {
                sak_key: SakKey::ArkivId("2025/99".to_string()),
                saksbehandler_id: "Z99999".to_string(),
                saksbehandler_enhet: "43".to_string(),
            })
        );
    }

    fn wire_envelope(command: WireCommand) -> WireCommandEnvelope<WireCommand> {
        WireCommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: command,
        }
    }

    fn journalpost_common(
        client_reference: Uuid,
        sak_key: WireSakKey,
        form: WireDokumentform,
        dokument_client_reference: Uuid,
    ) -> WireJournalpostCommon {
        WireJournalpostCommon {
            client_reference,
            tittel: "Journalpost".to_string(),
            dokument_dato: "2025-01-01".to_string(),
            saksbehandler: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: Some(wire_tilgang()),
            dokumenter: vec![WireDokument {
                client_reference: dokument_client_reference,
                tittel: "Dokument".to_string(),
                form,
            }],
            sak_key,
            kildesystem: Some("test".to_string()),
        }
    }

    fn wire_tilgang() -> WireTilgang {
        WireTilgang {
            tilgangskode: "UO".to_string(),
            tilgangshjemmel: "offl".to_string(),
        }
    }

    fn assert_metadata(
        envelope: &CommandEnvelope<ApplicationCommand>,
        command_id: uuid::Uuid,
        correlation_id: Option<uuid::Uuid>,
    ) {
        assert_eq!(envelope.command_id, command_id);
        assert_eq!(envelope.correlation_id, correlation_id);
    }

    fn assert_journalpost_common(
        common: &JournalpostCommon,
        journalpost_ref: Uuid,
        sak_key: SakKey,
        dokument_ref: Uuid,
    ) {
        assert_eq!(common.client_reference, journalpost_ref);
        assert_eq!(common.tittel, "Journalpost");
        assert_eq!(common.dokument_dato, "2025-01-01");
        assert_eq!(common.saksbehandler, "Z12345");
        assert_eq!(common.saksbehandler_enhet, "42");
        assert_eq!(
            common.tilgang,
            Some(Tilgang {
                tilgangskode: "UO".to_string(),
                tilgangshjemmel: "offl".to_string()
            })
        );
        assert_eq!(common.sak_key, sak_key);
        assert_eq!(common.kildesystem.as_deref(), Some("test"));
        assert_eq!(common.dokumenter.len(), 1);
        assert_eq!(common.dokumenter[0].client_reference, dokument_ref);
        assert_eq!(common.dokumenter[0].tittel, "Dokument");
    }

    fn assert_template_document(document: &Dokument, mal_ref: Uuid) {
        match &document.form {
            Dokumentform::HtmlTemplate {
                mal_referanse,
                felter,
            } => {
                assert_eq!(*mal_referanse, mal_ref);
                assert_eq!(felter, &[TemplateFelt::Saksnummer]);
            }
            other => panic!("expected template document, got {other:?}"),
        }
    }
}
