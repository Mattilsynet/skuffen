use application::command::{
    Arkivdel, AvsluttSakCommand, Command as ApplicationCommand, CommandEnvelope, Dokument,
    Dokumentform, JournalpostCommon, Korrespondansepart, MottakerId, OpprettJournalpostCommand,
    OpprettSakCommand, Parttype, Postadresse, SakKey, SettSaksansvarligCommand, Tilgjengelighet,
    Utsendingsmottaker,
};
use domain::eksekvering::html_template::TemplateFelt;
use lib_schemas::skuffen::command::commands::{
    Command as WireCommand, CommandEnvelope as WireCommandEnvelope,
};
use lib_schemas::skuffen::command::journalpost as wire_journalpost;
use lib_schemas::skuffen::command::sak as wire_sak;
use lib_schemas::skuffen::dokument as wire_dokument;
use lib_schemas::skuffen::query::queries as wire_query;
use lib_schemas::skuffen::tilgang as wire_tilgang;
use lib_schemas::typer::organisasjonsnummer::Organisasjonsnummer;
use lib_schemas::typer::personnummer::Personnummer;

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
) -> anyhow::Result<WireCommandEnvelope<WireCommand>> {
    Ok(WireCommandEnvelope {
        command_id: envelope.command_id,
        correlation_id: envelope.correlation_id,
        payload: map_application_command_to_wire(&envelope.payload)?,
    })
}

fn map_application_command_to_wire(command: &ApplicationCommand) -> anyhow::Result<WireCommand> {
    match command {
        ApplicationCommand::OpprettSak(command) => {
            Ok(WireCommand::OpprettSak(wire_sak::OpprettSak {
                client_reference: command.client_reference,
                sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(
                    command.sakstittel.clone(),
                )?,
                ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new(
                    command.ordningsverdi.get().to_string(),
                )?,
                arkivdel: map_application_arkivdel(command.arkivdel),
                saksbehandler_id: command.saksbehandler_id.clone(),
                saksbehandler_enhet: command.saksbehandler_enhet.clone(),
                tilgjengelighet: map_application_tilgjengelighet(&command.tilgjengelighet)?,
            }))
        }
        ApplicationCommand::OpprettInngaaendeJournalpost(command) => {
            let OpprettJournalpostCommand::Inngaende { felles, avsender } = command else {
                anyhow::bail!("OpprettInngaaendeJournalpost bærer feil journalpost-variant");
            };
            Ok(WireCommand::OpprettInngåendeJournalpost(
                wire_journalpost::OpprettInngåendeJournalpost {
                    felles: map_application_journalpost_common(felles)?,
                    avsender: map_application_korrespondansepart(avsender),
                },
            ))
        }
        ApplicationCommand::OpprettUtgaaendeJournalpost(command) => match command {
            OpprettJournalpostCommand::Utgaaende { felles, mottakere } => {
                Ok(WireCommand::OpprettUtgåendeJournalpost(
                    wire_journalpost::OpprettUtgåendeJournalpost {
                        felles: map_application_journalpost_common(felles)?,
                        mottakere: mottakere
                            .iter()
                            .map(map_application_korrespondansepart)
                            .collect(),
                    },
                ))
            }
            OpprettJournalpostCommand::UtgaaendeMedUtsending { felles, mottakere } => {
                let mottakere = mottakere
                    .iter()
                    .map(map_application_utsendingsmottaker)
                    .collect::<anyhow::Result<Vec<_>>>()?;
                Ok(WireCommand::OpprettUtgåendeJournalpostMedUtsending(
                    wire_journalpost::OpprettUtgåendeJournalpostMedUtsending {
                        felles: map_application_journalpost_common(felles)?,
                        mottakere,
                    },
                ))
            }
            _ => anyhow::bail!("OpprettUtgaaendeJournalpost bærer feil journalpost-variant"),
        },
        ApplicationCommand::OpprettInterntNotatJournalpost(command) => {
            let OpprettJournalpostCommand::InterntNotat { felles } = command else {
                anyhow::bail!("OpprettInterntNotatJournalpost bærer feil journalpost-variant");
            };
            Ok(WireCommand::OpprettInterntNotatJournalpost(
                wire_journalpost::OpprettInterntNotatJournalpost {
                    felles: map_application_journalpost_common(felles)?,
                },
            ))
        }
        ApplicationCommand::AvsluttSak(command) => {
            Ok(WireCommand::AvsluttSak(wire_sak::AvsluttSak {
                sak_key: map_application_sak_key(&command.sak_key)?,
            }))
        }
        ApplicationCommand::SettSaksansvarlig(command) => Ok(WireCommand::SettSaksansvarlig(
            wire_sak::SettSaksansvarlig {
                sak_key: map_application_sak_key(&command.sak_key)?,
                saksbehandler_id: command.saksbehandler_id.clone(),
                saksbehandler_enhet: command.saksbehandler_enhet.clone(),
            },
        )),
    }
}

fn map_application_journalpost_common(
    command: &JournalpostCommon,
) -> anyhow::Result<wire_journalpost::JournalpostCommon> {
    Ok(wire_journalpost::JournalpostCommon {
        client_reference: command.client_reference,
        tittel: command.tittel.clone(),
        dokument_dato: command.dokument_dato.clone(),
        saksbehandler: command.saksbehandler.clone(),
        saksbehandler_enhet: command.saksbehandler_enhet.clone(),
        tilgjengelighet: map_application_tilgjengelighet(&command.tilgjengelighet)?,
        dokumenter: command
            .dokumenter
            .iter()
            .map(map_application_dokument)
            .collect(),
        sak_key: map_application_sak_key(&command.sak_key)?,
        kildesystem: command.kildesystem.clone(),
    })
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

fn map_application_tilgjengelighet(
    tilgjengelighet: &Tilgjengelighet,
) -> anyhow::Result<wire_tilgang::Tilgjengelighet> {
    match tilgjengelighet {
        Tilgjengelighet::Offentlig => Ok(wire_tilgang::Tilgjengelighet::Offentlig),
        Tilgjengelighet::Skjermet {
            tilgangskode,
            tilgangshjemmel,
        } => Ok(wire_tilgang::Tilgjengelighet::Skjermet {
            tilgangskode: wire_tilgang::Tilgangskode::new(tilgangskode.as_str())?,
            tilgangshjemmel: wire_tilgang::Tilgangshjemmel::new(tilgangshjemmel.as_str())?,
        }),
    }
}

fn map_application_parttype(parttype: Parttype) -> wire_journalpost::Parttype {
    match parttype {
        Parttype::Person => wire_journalpost::Parttype::Person,
        Parttype::Virksomhet => wire_journalpost::Parttype::Virksomhet,
    }
}

fn map_application_korrespondansepart(
    part: &Korrespondansepart,
) -> wire_journalpost::Korrespondansepart {
    wire_journalpost::Korrespondansepart {
        navn: part.navn.clone(),
        parttype: map_application_parttype(part.parttype),
    }
}

fn map_application_utsendingsmottaker(
    mottaker: &Utsendingsmottaker,
) -> anyhow::Result<wire_journalpost::Utsendingsmottaker> {
    let id = match &mottaker.id {
        MottakerId::Person { fødselsnummer } => wire_journalpost::MottakerId::Person {
            fødselsnummer: Personnummer::new(fødselsnummer.as_str()).map_err(|e| {
                anyhow::anyhow!("intern fødselsnummer kunne ikke konverteres til wire: {e}")
            })?,
        },
        MottakerId::Virksomhet {
            organisasjonsnummer,
        } => wire_journalpost::MottakerId::Virksomhet {
            organisasjonsnummer: Organisasjonsnummer::new(organisasjonsnummer.as_str()).map_err(
                |e| {
                    anyhow::anyhow!(
                        "intern organisasjonsnummer kunne ikke konverteres til wire: {e}"
                    )
                },
            )?,
        },
    };
    Ok(wire_journalpost::Utsendingsmottaker {
        navn: mottaker.navn.clone(),
        id,
        adresse: wire_journalpost::Postadresse {
            adresse: mottaker.adresse.adresse.clone(),
            postnummer: lib_schemas::skuffen::journalpost::Postnummer::new(
                mottaker.adresse.postnummer.as_str(),
            )?,
            poststed: mottaker.adresse.poststed.clone(),
        },
    })
}

fn map_application_sak_key(sak_key: &SakKey) -> anyhow::Result<wire_query::SakKey> {
    match sak_key {
        SakKey::ClientReference(client_reference) => {
            Ok(wire_query::SakKey::ClientReference(*client_reference))
        }
        SakKey::ArkivId(saksnummer) => Ok(wire_query::SakKey::ArkivId(
            lib_schemas::skuffen::sak::Saksnummer::new(saksnummer.clone())?,
        )),
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
        WireCommand::OpprettUtgåendeJournalpostMedUtsending(command) => {
            ApplicationCommand::OpprettUtgaaendeJournalpost(map_utgaaende_med_utsending(command))
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

fn map_tilgjengelighet(tilgjengelighet: wire_tilgang::Tilgjengelighet) -> Tilgjengelighet {
    match tilgjengelighet {
        wire_tilgang::Tilgjengelighet::Offentlig => Tilgjengelighet::Offentlig,
        wire_tilgang::Tilgjengelighet::Skjermet {
            tilgangskode,
            tilgangshjemmel,
        } => Tilgjengelighet::Skjermet {
            tilgangskode: domain::model::tilgang::Tilgangskode::new(tilgangskode.as_str())
                .expect("wire tilgangskode er validert av serde"),
            tilgangshjemmel: domain::model::tilgang::Tilgangshjemmel::new(tilgangshjemmel.as_str())
                .expect("wire tilgangshjemmel er validert av serde"),
        },
    }
}

fn map_parttype(parttype: wire_journalpost::Parttype) -> Parttype {
    match parttype {
        wire_journalpost::Parttype::Person => Parttype::Person,
        wire_journalpost::Parttype::Virksomhet => Parttype::Virksomhet,
    }
}

fn map_korrespondansepart(part: wire_journalpost::Korrespondansepart) -> Korrespondansepart {
    Korrespondansepart {
        navn: part.navn,
        parttype: map_parttype(part.parttype),
    }
}

fn map_utsendingsmottaker(mottaker: wire_journalpost::Utsendingsmottaker) -> Utsendingsmottaker {
    Utsendingsmottaker {
        navn: mottaker.navn,
        id: match mottaker.id {
            wire_journalpost::MottakerId::Person { fødselsnummer } => MottakerId::Person {
                fødselsnummer: domain::model::identifikator::Fødselsnummer::new(
                    fødselsnummer.as_str(),
                )
                .expect("wire fødselsnummer er validert av serde"),
            },
            wire_journalpost::MottakerId::Virksomhet {
                organisasjonsnummer,
            } => MottakerId::Virksomhet {
                organisasjonsnummer: domain::model::identifikator::Organisasjonsnummer::new(
                    organisasjonsnummer.as_str(),
                )
                .expect("wire organisasjonsnummer er validert av serde"),
            },
        },
        adresse: Postadresse {
            adresse: mottaker.adresse.adresse,
            postnummer: domain::model::identifikator::Postnummer::new(
                mottaker.adresse.postnummer.as_str(),
            )
            .expect("wire postnummer er validert av serde"),
            poststed: mottaker.adresse.poststed,
        },
    }
}

fn map_opprett_sak(command: wire_sak::OpprettSak) -> OpprettSakCommand {
    OpprettSakCommand {
        client_reference: command.client_reference,
        sakstittel: command.sakstittel.as_str().to_string(),
        ordningsverdi: domain::model::sak::Ordningsverdi::new(
            command.ordningsverdi.as_str().to_string(),
        )
        .expect("wire ordningsverdi er validert av serde"),
        arkivdel: map_arkivdel(command.arkivdel),
        saksbehandler_id: command.saksbehandler_id,
        saksbehandler_enhet: command.saksbehandler_enhet,
        tilgjengelighet: map_tilgjengelighet(command.tilgjengelighet),
    }
}

fn map_inngaende_journalpost(
    command: wire_journalpost::OpprettInngåendeJournalpost,
) -> OpprettJournalpostCommand {
    OpprettJournalpostCommand::Inngaende {
        felles: map_journalpost_common(command.felles),
        avsender: map_korrespondansepart(command.avsender),
    }
}

fn map_utgaaende_journalpost(
    command: wire_journalpost::OpprettUtgåendeJournalpost,
) -> OpprettJournalpostCommand {
    OpprettJournalpostCommand::Utgaaende {
        felles: map_journalpost_common(command.felles),
        mottakere: command
            .mottakere
            .into_iter()
            .map(map_korrespondansepart)
            .collect(),
    }
}

fn map_utgaaende_med_utsending(
    command: wire_journalpost::OpprettUtgåendeJournalpostMedUtsending,
) -> OpprettJournalpostCommand {
    OpprettJournalpostCommand::UtgaaendeMedUtsending {
        felles: map_journalpost_common(command.felles),
        mottakere: command
            .mottakere
            .into_iter()
            .map(map_utsendingsmottaker)
            .collect(),
    }
}

fn map_internt_notat_journalpost(
    command: wire_journalpost::OpprettInterntNotatJournalpost,
) -> OpprettJournalpostCommand {
    OpprettJournalpostCommand::InterntNotat {
        felles: map_journalpost_common(command.felles),
    }
}

fn map_journalpost_common(command: wire_journalpost::JournalpostCommon) -> JournalpostCommon {
    JournalpostCommon {
        client_reference: command.client_reference,
        tittel: command.tittel,
        dokument_dato: command.dokument_dato,
        saksbehandler: command.saksbehandler,
        saksbehandler_enhet: command.saksbehandler_enhet,
        tilgjengelighet: map_tilgjengelighet(command.tilgjengelighet),
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
    use application::command::{
        Korrespondansepart, MottakerId, Parttype, Postadresse, Utsendingsmottaker,
    };
    use lib_schemas::skuffen::command::commands::Command as WireCommand;
    use lib_schemas::skuffen::command::journalpost::{
        JournalpostCommon as WireJournalpostCommon, Korrespondansepart as WireKorrespondansepart,
        MottakerId as WireMottakerId, OpprettInngåendeJournalpost, OpprettInterntNotatJournalpost,
        OpprettUtgåendeJournalpost, OpprettUtgåendeJournalpostMedUtsending,
        Parttype as WireParttype, Postadresse as WirePostadresse,
        Utsendingsmottaker as WireUtsendingsmottaker,
    };
    use lib_schemas::skuffen::command::sak::{
        Arkivdel as WireArkivdel, AvsluttSak, OpprettSak, SettSaksansvarlig,
    };
    use lib_schemas::skuffen::dokument::{
        Dokument as WireDokument, Dokumentform as WireDokumentform, Felt,
    };
    use lib_schemas::skuffen::journalpost::Postnummer as WirePostnummer;
    use lib_schemas::skuffen::query::queries::SakKey as WireSakKey;
    use lib_schemas::skuffen::sak::{Ordningsverdi, Saksnummer, Sakstittel};
    use lib_schemas::skuffen::tilgang::{
        Tilgangshjemmel, Tilgangskode, Tilgjengelighet as WireTilgjengelighet,
    };
    use lib_schemas::typer::organisasjonsnummer::Organisasjonsnummer;
    use uuid::Uuid;

    #[test]
    fn maps_opprett_sak_preserving_metadata_and_sak_fields() {
        let client_reference = Uuid::new_v4();
        let envelope = wire_envelope(WireCommand::OpprettSak(OpprettSak {
            client_reference,
            sakstittel: Sakstittel::try_from("Test sak".to_string()).unwrap(),
            ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
            arkivdel: WireArkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: wire_skjermet(),
        }));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        match mapped.payload {
            ApplicationCommand::OpprettSak(command) => {
                assert_eq!(command.client_reference, client_reference);
                assert_eq!(command.sakstittel, "Test sak");
                assert_eq!(command.ordningsverdi.get(), "123");
                assert_eq!(command.arkivdel, Arkivdel::Tilsynsdivisjonene);
                assert_eq!(command.saksbehandler_id, "Z12345");
                assert_eq!(command.saksbehandler_enhet, "42");
                assert_eq!(command.tilgjengelighet, application_skjermet());
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
                avsender: WireKorrespondansepart {
                    navn: "Avsender".to_string(),
                    parttype: WireParttype::Person,
                },
            },
        ));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        match mapped.payload {
            ApplicationCommand::OpprettInngaaendeJournalpost(
                OpprettJournalpostCommand::Inngaende { felles, avsender },
            ) => {
                assert_journalpost_common(
                    &felles,
                    journalpost_ref,
                    SakKey::ClientReference(sak_ref),
                    dokument_ref,
                );
                assert_eq!(
                    avsender,
                    Korrespondansepart {
                        navn: "Avsender".to_string(),
                        parttype: Parttype::Person,
                    }
                );
                assert_template_document(&felles.dokumenter[0], mal_ref);
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
            OpprettUtgåendeJournalpost {
                felles: journalpost_common(
                    journalpost_ref,
                    WireSakKey::ArkivId(saksnummer),
                    WireDokumentform::HtmlTemplate {
                        mal_referanse: mal_ref,
                        felter: vec![Felt::Saksnummer],
                    },
                    dokument_ref,
                ),
                mottakere: vec![WireKorrespondansepart {
                    navn: "Mottaker".to_string(),
                    parttype: WireParttype::Virksomhet,
                }],
            },
        ));
        let command_id = envelope.command_id;
        let correlation_id = envelope.correlation_id;

        let mapped = map_wire_envelope(envelope);

        assert_metadata(&mapped, command_id, correlation_id);
        match mapped.payload {
            ApplicationCommand::OpprettUtgaaendeJournalpost(
                OpprettJournalpostCommand::Utgaaende { felles, mottakere },
            ) => {
                assert_journalpost_common(
                    &felles,
                    journalpost_ref,
                    SakKey::ArkivId("2025/42".to_string()),
                    dokument_ref,
                );
                assert_eq!(
                    mottakere,
                    vec![Korrespondansepart {
                        navn: "Mottaker".to_string(),
                        parttype: Parttype::Virksomhet,
                    }]
                );
                assert_template_document(&felles.dokumenter[0], mal_ref);
            }
            other => panic!("expected OpprettUtgaaendeJournalpost, got {other:?}"),
        }
    }

    #[test]
    fn maps_utgaaende_med_utsending_preserving_mottakere_and_flag() {
        let journalpost_ref = Uuid::new_v4();
        let sak_ref = Uuid::new_v4();
        let dokument_ref = Uuid::new_v4();
        let envelope = wire_envelope(WireCommand::OpprettUtgåendeJournalpostMedUtsending(
            OpprettUtgåendeJournalpostMedUtsending {
                felles: journalpost_common(
                    journalpost_ref,
                    WireSakKey::ClientReference(sak_ref),
                    WireDokumentform::Bytes {
                        dokument_referanse: dokument_ref,
                        filtype: "PDF".to_string(),
                    },
                    dokument_ref,
                ),
                mottakere: vec![WireUtsendingsmottaker {
                    navn: "Bedrift AS".to_string(),
                    id: WireMottakerId::Virksomhet {
                        organisasjonsnummer: Organisasjonsnummer::new("995298775").unwrap(),
                    },
                    adresse: WirePostadresse {
                        adresse: "Storgata 1".to_string(),
                        postnummer: WirePostnummer::new("0350").unwrap(),
                        poststed: "Oslo".to_string(),
                    },
                }],
            },
        ));

        let mapped = map_wire_envelope(envelope);

        match mapped.payload {
            ApplicationCommand::OpprettUtgaaendeJournalpost(
                OpprettJournalpostCommand::UtgaaendeMedUtsending { mottakere, .. },
            ) => {
                assert_eq!(
                    mottakere,
                    vec![Utsendingsmottaker {
                        navn: "Bedrift AS".to_string(),
                        id: MottakerId::Virksomhet {
                            organisasjonsnummer:
                                domain::model::identifikator::Organisasjonsnummer::new("995298775")
                                    .unwrap(),
                        },
                        adresse: Postadresse {
                            adresse: "Storgata 1".to_string(),
                            postnummer: domain::model::identifikator::Postnummer::new("0350")
                                .unwrap(),
                            poststed: "Oslo".to_string(),
                        },
                    }]
                );
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
            ApplicationCommand::OpprettInterntNotatJournalpost(
                OpprettJournalpostCommand::InterntNotat { felles },
            ) => {
                assert_journalpost_common(
                    &felles,
                    journalpost_ref,
                    SakKey::ClientReference(sak_ref),
                    dokument_ref,
                );
                match &felles.dokumenter[0].form {
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

    #[test]
    fn med_utsending_round_trips_back_to_wire_variant() {
        let envelope = CommandEnvelope::new(
            Uuid::new_v4(),
            ApplicationCommand::OpprettUtgaaendeJournalpost(
                application::command::OpprettJournalpostCommand::UtgaaendeMedUtsending {
                    felles: application_journalpost_common(),
                    mottakere: vec![Utsendingsmottaker {
                        navn: "Bedrift AS".to_string(),
                        id: MottakerId::Virksomhet {
                            organisasjonsnummer:
                                domain::model::identifikator::Organisasjonsnummer::new("995298775")
                                    .unwrap(),
                        },
                        adresse: Postadresse {
                            adresse: "Storgata 1".to_string(),
                            postnummer: domain::model::identifikator::Postnummer::new("0350")
                                .unwrap(),
                            poststed: "Oslo".to_string(),
                        },
                    }],
                },
            ),
        );

        let wire = map_application_envelope_to_wire(&envelope).unwrap();
        assert!(matches!(
            wire.payload,
            WireCommand::OpprettUtgåendeJournalpostMedUtsending(_)
        ));
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
            tilgjengelighet: wire_skjermet(),
            dokumenter: vec![WireDokument {
                client_reference: dokument_client_reference,
                tittel: "Dokument".to_string(),
                form,
            }],
            sak_key,
            kildesystem: Some("test".to_string()),
        }
    }

    fn application_journalpost_common() -> JournalpostCommon {
        JournalpostCommon {
            client_reference: Uuid::new_v4(),
            tittel: "Journalpost".to_string(),
            dokument_dato: "2025-01-01".to_string(),
            saksbehandler: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: application_skjermet(),
            dokumenter: Vec::new(),
            sak_key: SakKey::ClientReference(Uuid::new_v4()),
            kildesystem: None,
        }
    }

    fn wire_skjermet() -> WireTilgjengelighet {
        WireTilgjengelighet::Skjermet {
            tilgangskode: Tilgangskode::new("UO").unwrap(),
            tilgangshjemmel: Tilgangshjemmel::new("offl").unwrap(),
        }
    }

    fn application_skjermet() -> Tilgjengelighet {
        Tilgjengelighet::Skjermet {
            tilgangskode: domain::model::tilgang::Tilgangskode::new("UO").unwrap(),
            tilgangshjemmel: domain::model::tilgang::Tilgangshjemmel::new("offl").unwrap(),
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
        assert_eq!(common.tilgjengelighet, application_skjermet());
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

    // Domenet og wire-kontrakten dupliserer bevisst valideringsreglene
    // (SKU-0013: domain importerer ikke lib_schemas). Denne testen sikrer at
    // de ikke drifter fra hverandre, slik at en verdi aldri er gyldig i ett lag
    // men avvist i det andre.
    #[test]
    fn fnr_validering_er_identisk_i_domene_og_wire() {
        let cases = [
            "01010101006",   // gyldig
            "995298775",     // for kort (9)
            "01010101007",   // feil kontrollsiffer
            "tull",          // ikke-numerisk
            "",              // tom
            "0101010100600", // for lang
        ];
        for c in cases {
            let domene = domain::model::identifikator::Fødselsnummer::new(c).is_ok();
            let wire = lib_schemas::typer::personnummer::Personnummer::new(c).is_ok();
            assert_eq!(domene, wire, "fnr-paritet avvik for '{c}'");
        }
    }

    #[test]
    fn orgnr_validering_er_identisk_i_domene_og_wire() {
        let cases = [
            "995298775",
            "995298776",
            "01010101006",
            "tull",
            "",
            "12345678",
        ];
        for c in cases {
            let domene = domain::model::identifikator::Organisasjonsnummer::new(c).is_ok();
            let wire = lib_schemas::typer::organisasjonsnummer::Organisasjonsnummer::new(c).is_ok();
            assert_eq!(domene, wire, "orgnr-paritet avvik for '{c}'");
        }
    }

    #[test]
    fn postnummer_validering_er_identisk_i_domene_og_wire() {
        let cases = ["0350", "035", "03500", "abcd", ""];
        for c in cases {
            let domene = domain::model::identifikator::Postnummer::new(c).is_ok();
            let wire = lib_schemas::skuffen::journalpost::Postnummer::new(c).is_ok();
            assert_eq!(domene, wire, "postnummer-paritet avvik for '{c}'");
        }
    }
}
