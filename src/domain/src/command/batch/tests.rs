use std::collections::HashSet;

use uuid::Uuid;

use super::*;

fn opprett_sak(sak: Uuid) -> Batchkommando {
    Batchkommando {
        command_id: Uuid::new_v4(),
        command_type: CommandTypeCode::OpprettSak,
        oppretter: vec![Opprettelse {
            entitet: Entitetsplass::Sak,
            client_reference: sak,
        }],
        sak_referanse: None,
    }
}

fn notat(sak: Uuid, journalpost: Uuid, dokument: Uuid) -> Batchkommando {
    Batchkommando {
        command_id: Uuid::new_v4(),
        command_type: CommandTypeCode::OpprettInterntNotatJournalpost,
        oppretter: vec![
            Opprettelse {
                entitet: Entitetsplass::Journalpost,
                client_reference: journalpost,
            },
            Opprettelse {
                entitet: Entitetsplass::Dokument(0),
                client_reference: dokument,
            },
        ],
        sak_referanse: Some(sak),
    }
}

fn avslutt(sak: Uuid) -> Batchkommando {
    Batchkommando {
        command_id: Uuid::new_v4(),
        command_type: CommandTypeCode::AvsluttSak,
        oppretter: Vec::new(),
        sak_referanse: Some(sak),
    }
}

fn kjent(client_references: impl IntoIterator<Item = Uuid>) -> KjentTilstand {
    KjentTilstand {
        client_references: client_references.into_iter().collect(),
        ..Default::default()
    }
}

#[test]
fn gyldig_batch_med_ny_sak_godtas() {
    let sak = Uuid::new_v4();
    let batch = [
        opprett_sak(sak),
        notat(sak, Uuid::new_v4(), Uuid::new_v4()),
        avslutt(sak),
    ];

    assert_eq!(valider_batch(&batch, &KjentTilstand::default()), Ok(()));
}

#[test]
fn referanse_til_sak_som_finnes_i_skuffen_godtas() {
    let sak = Uuid::new_v4();
    let batch = [notat(sak, Uuid::new_v4(), Uuid::new_v4()), avslutt(sak)];

    assert_eq!(valider_batch(&batch, &kjent([sak])), Ok(()));
}

/// Batchen som slapp gjennom i test: én UUID brukt til alt.
#[test]
fn samme_uuid_overalt_gir_alle_feilene() {
    let id = Uuid::new_v4();
    let mut sak = opprett_sak(id);
    sak.command_id = id;
    let mut journalpost = notat(id, id, id);
    journalpost.command_id = id;
    let mut avslutning = avslutt(id);
    avslutning.command_id = id;

    let feil = valider_batch(&[sak, journalpost, avslutning], &KjentTilstand::default())
        .expect_err("batchen skal avvises");

    assert_eq!(
        feil,
        vec![
            Batchfeil::CommandIdGjentatt {
                command_id: id,
                posisjoner: vec![0, 1, 2],
            },
            Batchfeil::ClientReferenceGjentatt {
                client_reference: id,
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
        ]
    );
}

#[test]
fn command_id_kjent_med_annen_type_avvises() {
    let sak = Uuid::new_v4();
    let kommando = avslutt(sak);
    let mut kjent = kjent([sak]);
    kjent
        .kommandoer
        .insert(kommando.command_id, CommandTypeCode::OpprettSak);

    assert_eq!(
        valider_batch(std::slice::from_ref(&kommando), &kjent),
        Err(vec![Batchfeil::CommandIdBruktAvAnnenKommandotype {
            posisjon: 0,
            command_id: kommando.command_id,
        }])
    );
}

#[test]
fn ny_kommando_kan_ikke_opprette_client_reference_som_finnes() {
    let sak = Uuid::new_v4();

    assert_eq!(
        valider_batch(&[opprett_sak(sak)], &kjent([sak])),
        Err(vec![Batchfeil::ClientReferenceFinnesFraFoer {
            forekomst: Forekomst {
                posisjon: 0,
                entitet: Entitetsplass::Sak,
            },
            client_reference: sak,
        }])
    );
}

#[test]
fn dokument_kan_ikke_gjenbruke_client_reference_fra_en_annen_entitet() {
    let sak = Uuid::new_v4();
    let gammel_journalpost = Uuid::new_v4();
    let kjent = kjent([sak, gammel_journalpost]);

    assert_eq!(
        valider_batch(&[notat(sak, Uuid::new_v4(), gammel_journalpost)], &kjent),
        Err(vec![Batchfeil::ClientReferenceFinnesFraFoer {
            forekomst: Forekomst {
                posisjon: 0,
                entitet: Entitetsplass::Dokument(0),
            },
            client_reference: gammel_journalpost,
        }])
    );
}

/// En klient som ikke fikk svar sender samme batch igjen. Det skal gi `Ok`.
#[test]
fn gjentatt_innsending_av_samme_batch_godtas() {
    let sak = Uuid::new_v4();
    let journalpost = Uuid::new_v4();
    let dokument = Uuid::new_v4();
    let batch = [
        opprett_sak(sak),
        notat(sak, journalpost, dokument),
        avslutt(sak),
    ];
    let kjent = KjentTilstand {
        kommandoer: batch
            .iter()
            .map(|kommando| (kommando.command_id, kommando.command_type))
            .collect(),
        client_references: HashSet::from([sak, journalpost, dokument]),
    };

    assert_eq!(valider_batch(&batch, &kjent), Ok(()));
}

/// Første forsøk feilet etter at saken var registrert. Batchen sendes på nytt
/// med de samme kommandoene.
#[test]
fn gjentatt_innsending_etter_delvis_mottak_godtas() {
    let sak = Uuid::new_v4();
    let batch = [opprett_sak(sak), notat(sak, Uuid::new_v4(), Uuid::new_v4())];
    let mut kjent = kjent([sak]);
    kjent
        .kommandoer
        .insert(batch[0].command_id, CommandTypeCode::OpprettSak);

    assert_eq!(valider_batch(&batch, &kjent), Ok(()));
}

#[test]
fn ukjent_sak_referanse_avvises() {
    let sak = Uuid::new_v4();

    assert_eq!(
        valider_batch(&[avslutt(sak)], &KjentTilstand::default()),
        Err(vec![Batchfeil::SakReferanseUkjent {
            posisjon: 0,
            client_reference: sak,
        }])
    );
}

#[test]
fn sak_som_opprettes_senere_i_batchen_avvises() {
    let sak = Uuid::new_v4();

    assert_eq!(
        valider_batch(&[avslutt(sak), opprett_sak(sak)], &KjentTilstand::default()),
        Err(vec![Batchfeil::SakReferanseUkjent {
            posisjon: 0,
            client_reference: sak,
        }])
    );
}
