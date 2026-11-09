use domain::command::batch::{Batchfeil, Entitetsplass, Forekomst};
use lib_schemas::skuffen::command::commands::CommandEnvelope;
use uuid::Uuid;

use super::*;

const NAVN: [&str; 3] = ["OpprettSak", "OpprettInterntNotatJournalpost", "AvsluttSak"];

fn id() -> Uuid {
    Uuid::parse_str("54e028bf-fdb6-489f-a7be-1449e2f04e5b").unwrap()
}

#[test]
fn batchen_fra_test_forklares_linje_for_linje() {
    let feil = [
        Batchfeil::CommandIdGjentatt {
            command_id: id(),
            posisjoner: vec![0, 1, 2],
        },
        Batchfeil::ClientReferenceGjentatt {
            client_reference: id(),
            forekomster: vec![
                Forekomst {
                    posisjon: 0,
                    entitet: Entitetsplass::Sak,
                },
                Forekomst {
                    posisjon: 1,
                    entitet: Entitetsplass::Journalpost,
                },
                Forekomst {
                    posisjon: 1,
                    entitet: Entitetsplass::Dokument(0),
                },
            ],
        },
    ];

    assert_eq!(
        ugyldig_batch(&feil, &NAVN),
        "Batchen ble avvist, og ingen kommandoer er tatt imot. 2 feil:\n\
         - command_id 54e028bf-fdb6-489f-a7be-1449e2f04e5b er brukt av kommando 1 (OpprettSak), \
         kommando 2 (OpprettInterntNotatJournalpost) og kommando 3 (AvsluttSak). \
         Hver kommando må ha sin egen command_id.\n\
         - client_reference 54e028bf-fdb6-489f-a7be-1449e2f04e5b brukes for saken i kommando 1 (OpprettSak), \
         journalposten i kommando 2 (OpprettInterntNotatJournalpost) og \
         dokument 1 i kommando 2 (OpprettInterntNotatJournalpost). \
         Hver sak, journalpost og hvert dokument må ha sin egen client_reference."
    );
}

#[test]
fn hver_batchfeil_nevner_kommandoen_den_gjelder() {
    let feil = [
        Batchfeil::CommandIdBruktAvAnnenKommandotype {
            posisjon: 2,
            command_id: id(),
        },
        Batchfeil::ClientReferenceFinnesFraFoer {
            forekomst: Forekomst {
                posisjon: 1,
                entitet: Entitetsplass::Dokument(0),
            },
            client_reference: id(),
        },
        Batchfeil::SakReferanseUkjent {
            posisjon: 1,
            client_reference: id(),
        },
    ];

    let melding = ugyldig_batch(&feil, &NAVN);
    let linjer: Vec<&str> = melding.lines().skip(1).collect();

    assert_eq!(linjer.len(), 3, "{melding}");
    assert!(
        linjer[0]
            .contains("i kommando 3 (AvsluttSak) er allerede tatt imot for en annen kommandotype")
    );
    assert!(linjer[1].contains(
        "for dokument 1 i kommando 2 (OpprettInterntNotatJournalpost) finnes allerede i Skuffen"
    ));
    assert!(linjer[2].contains(
        "sak_key i kommando 2 (OpprettInterntNotatJournalpost) viser til client_reference"
    ));
}

#[test]
fn manglende_fil_forklares_med_dokument_og_kommando() {
    let feil = [Mediafeil {
        posisjon: 1,
        dokument: 0,
        art: MediafeilArt::DokumentIkkeLastetOpp {
            dokument_referanse: id(),
        },
    }];

    assert_eq!(
        manglende_media(&feil, &NAVN),
        "Batchen ble avvist, og ingen kommandoer er tatt imot. 1 feil:\n\
         - Dokument 1 i kommando 2 (OpprettInterntNotatJournalpost) viser til dokument_referanse \
         54e028bf-fdb6-489f-a7be-1449e2f04e5b, som ikke er lastet opp. Last opp filen via \
         arkiv.arkiver.media og vent på bekreftet lagring før batchen sendes."
    );
}

#[test]
fn ugyldig_json_sendes_videre_fra_parseren() {
    let feil = serde_json::from_str::<Vec<CommandEnvelope<Command>>>("[\n  {\"command_id\": }\n]")
        .expect_err("payload skal være ugyldig");

    assert_eq!(
        ugyldig_payload(&feil),
        "Batchen ble avvist, og ingen kommandoer er tatt imot. \
         Innholdet kunne ikke leses som en kommandobatch: expected value at line 2 column 18."
    );
}
