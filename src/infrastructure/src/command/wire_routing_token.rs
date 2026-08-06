use lib_schemas::skuffen::command::commands::Command;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStreamStage {
    Inbox,
    Ready,
    Done,
}

impl CommandStreamStage {
    fn as_wire_token(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Ready => "ready",
            Self::Done => "done",
        }
    }
}

pub fn routing_token_from_wire_command(command: &Command) -> &'static str {
    match command {
        Command::OpprettSak(_) | Command::AvsluttSak(_) | Command::SettSaksansvarlig(_) => "sak",
        Command::OpprettInngåendeJournalpost(_)
        | Command::OpprettUtgåendeJournalpost(_)
        | Command::OpprettUtgåendeJournalpostMedUtsending(_)
        | Command::OpprettInterntNotatJournalpost(_) => "journalpost",
    }
}

pub fn command_subject(stage: CommandStreamStage, command: &Command, command_id: Uuid) -> String {
    format!(
        "arkiv.command.{}.{}.{}",
        stage.as_wire_token(),
        routing_token_from_wire_command(command),
        command_id
    )
}

#[cfg(test)]
mod tests {
    use super::{CommandStreamStage, command_subject, routing_token_from_wire_command};
    use lib_schemas::skuffen::command::commands::Command;
    use lib_schemas::skuffen::command::journalpost::{
        JournalpostCommon, Korrespondansepart, OpprettInngåendeJournalpost,
        OpprettInterntNotatJournalpost, OpprettUtgåendeJournalpost,
        OpprettUtgåendeJournalpostMedUtsending, Parttype, Utsendingsmottaker,
    };
    use lib_schemas::skuffen::command::journalpost::{MottakerId, Postadresse};
    use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak, SettSaksansvarlig};
    use lib_schemas::skuffen::journalpost::Postnummer;
    use lib_schemas::skuffen::query::queries::SakKey;
    use lib_schemas::skuffen::sak::{Ordningsverdi, Sakstittel};
    use lib_schemas::skuffen::tilgang::Tilgjengelighet;
    use lib_schemas::typer::organisasjonsnummer::Organisasjonsnummer;
    use uuid::Uuid;

    fn fixed_uuid(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("123e4567-e89b-12d3-a456-42661417{suffix:04}"))
            .expect("valid fixed uuid")
    }

    fn sak_key() -> SakKey {
        SakKey::ClientReference(fixed_uuid(1))
    }

    fn journalpost_common(client_reference: Uuid) -> JournalpostCommon {
        JournalpostCommon {
            client_reference,
            tittel: "Journalpost".to_string(),
            dokument_dato: "2026-01-01".to_string(),
            saksbehandler: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: Tilgjengelighet::Offentlig,
            dokumenter: Vec::new(),
            sak_key: sak_key(),
            kildesystem: None,
        }
    }

    fn command_cases() -> [(Uuid, Command, &'static str); 7] {
        [
            (
                fixed_uuid(100),
                Command::OpprettSak(OpprettSak {
                    client_reference: fixed_uuid(1),
                    sakstittel: Sakstittel("Test sak".to_string()),
                    arkivdel: Arkivdel::Tilsynsdivisjonene,
                    saksbehandler_id: "Z12345".to_string(),
                    saksbehandler_enhet: "42".to_string(),
                    ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
                }),
                "sak",
            ),
            (
                fixed_uuid(101),
                Command::AvsluttSak(AvsluttSak { sak_key: sak_key() }),
                "sak",
            ),
            (
                fixed_uuid(102),
                Command::SettSaksansvarlig(SettSaksansvarlig {
                    sak_key: sak_key(),
                    saksbehandler_id: "Z12345".to_string(),
                    saksbehandler_enhet: "42".to_string(),
                }),
                "sak",
            ),
            (
                fixed_uuid(200),
                Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
                    felles: journalpost_common(fixed_uuid(2)),
                    avsender: Korrespondansepart {
                        navn: "Avsender".to_string(),
                        parttype: Parttype::Virksomhet,
                    },
                }),
                "journalpost",
            ),
            (
                fixed_uuid(201),
                Command::OpprettUtgåendeJournalpost(OpprettUtgåendeJournalpost {
                    felles: journalpost_common(fixed_uuid(3)),
                    mottakere: vec![Korrespondansepart {
                        navn: "Mottaker".to_string(),
                        parttype: Parttype::Virksomhet,
                    }],
                }),
                "journalpost",
            ),
            (
                fixed_uuid(203),
                Command::OpprettUtgåendeJournalpostMedUtsending(
                    OpprettUtgåendeJournalpostMedUtsending {
                        felles: journalpost_common(fixed_uuid(5)),
                        mottakere: vec![Utsendingsmottaker {
                            navn: "Bedrift AS".to_string(),
                            id: MottakerId::Virksomhet {
                                organisasjonsnummer: Organisasjonsnummer::new("995298775").unwrap(),
                            },
                            adresse: Postadresse {
                                adresse: "Storgata 1".to_string(),
                                postnummer: Postnummer::new("0350").unwrap(),
                                poststed: "Oslo".to_string(),
                            },
                        }],
                    },
                ),
                "journalpost",
            ),
            (
                fixed_uuid(202),
                Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                    felles: journalpost_common(fixed_uuid(4)),
                }),
                "journalpost",
            ),
        ]
    }

    #[test]
    fn routing_token_is_pinned_for_all_wire_command_variants() {
        for (_, command, expected_token) in command_cases() {
            assert_eq!(routing_token_from_wire_command(&command), expected_token);
        }
    }

    #[test]
    fn subject_shape_is_pinned_with_literal_wire_subjects() {
        for (command_id, command, expected_token) in command_cases() {
            let expected_inbox = format!("arkiv.command.inbox.{expected_token}.{command_id}");
            let expected_ready = format!("arkiv.command.ready.{expected_token}.{command_id}");
            let expected_done = format!("arkiv.command.done.{expected_token}.{command_id}");

            assert_eq!(
                command_subject(CommandStreamStage::Inbox, &command, command_id),
                expected_inbox
            );
            assert_eq!(
                command_subject(CommandStreamStage::Ready, &command, command_id),
                expected_ready
            );
            assert_eq!(
                command_subject(CommandStreamStage::Done, &command, command_id),
                expected_done
            );
        }
    }
}
