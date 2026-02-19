use lib_schemas::skuffen::command::commands::Command;
use lib_schemas::skuffen::command::journalpost::JournalpostCommon;
use lib_schemas::skuffen::query::queries::SakKey;
use uuid::Uuid;

use crate::eksekvering::typer::EksekveringFeil;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalpostType {
    Inngaende,
    Utgaaende,
    InterntNotat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Utsending {
    MedUtsending,
    UtenUtsending,
}

#[derive(Debug, Clone)]
pub struct JournalpostPlan {
    pub journalpost_id: Uuid,
    pub sak_key: SakKey,
    pub journalpost_type: JournalpostType,
    pub utsending: Option<Utsending>,
    pub dokumenter: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub enum Steg {
    OpprettSak {
        sak_id: Uuid,
    },
    OpprettJournalpost {
        plan: JournalpostPlan,
    },
    LeggTilDokument {
        journalpost_id: Uuid,
        dokument_id: Uuid,
    },
    Journalfoer {
        journalpost_id: Uuid,
    },
    Avskriv {
        journalpost_id: Uuid,
    },
    AvsluttSak {
        sak_id: Uuid,
    },
}

#[derive(Debug, Clone)]
pub struct EksekveringsPlan {
    pub steg: Vec<Steg>,
}

impl EksekveringsPlan {
    pub fn fra_command(command: &Command) -> Result<Self, EksekveringFeil> {
        match command {
            Command::OpprettSak(cmd) => Ok(Self {
                steg: vec![Steg::OpprettSak {
                    sak_id: cmd.client_reference,
                }],
            }),
            Command::AvsluttSak(cmd) => match cmd.sak_key.clone() {
                SakKey::ClientReference(sak_id) => Ok(Self {
                    steg: vec![Steg::AvsluttSak { sak_id }],
                }),
                SakKey::ArkivId(_) => Err(EksekveringFeil::blocked(
                    "Sak kan ikke avsluttes uten skuffen-id",
                )),
            },
            Command::OpprettInngåendeJournalpost(cmd) => {
                Self::valider_felles(&cmd.felles)?;
                if cmd.avsender.trim().is_empty() {
                    return Err(EksekveringFeil::irrecoverable(
                        "Inngående journalpost krever avsender",
                    ));
                }
                Self::journalpost_plan(
                    cmd.felles.client_reference,
                    cmd.felles.sak_key.clone(),
                    JournalpostType::Inngaende,
                    None,
                    &cmd.felles.dokumenter,
                )
            }
            Command::OpprettUtgåendeJournalpost(cmd) => {
                Self::valider_felles(&cmd.felles)?;
                if cmd.mottaker.trim().is_empty() {
                    return Err(EksekveringFeil::irrecoverable(
                        "Utgående journalpost krever mottaker",
                    ));
                }
                let utsending = Utsending::UtenUtsending;
                Self::journalpost_plan(
                    cmd.felles.client_reference,
                    cmd.felles.sak_key.clone(),
                    JournalpostType::Utgaaende,
                    Some(utsending),
                    &cmd.felles.dokumenter,
                )
            }
            Command::OpprettInterntNotatJournalpost(cmd) => {
                Self::valider_felles(&cmd.felles)?;
                Self::journalpost_plan(
                    cmd.felles.client_reference,
                    cmd.felles.sak_key.clone(),
                    JournalpostType::InterntNotat,
                    None,
                    &cmd.felles.dokumenter,
                )
            }
        }
    }

    fn valider_felles(felles: &JournalpostCommon) -> Result<(), EksekveringFeil> {
        if felles.tittel.trim().is_empty() {
            return Err(EksekveringFeil::irrecoverable("Journalpost krever tittel"));
        }
        if felles.dokument_dato.trim().is_empty() {
            return Err(EksekveringFeil::irrecoverable(
                "Journalpost krever dokumentdato",
            ));
        }
        if felles.saksbehandler.trim().is_empty() {
            return Err(EksekveringFeil::irrecoverable(
                "Journalpost krever saksbehandler",
            ));
        }
        if felles.saksbehandler_enhet.trim().is_empty() {
            return Err(EksekveringFeil::irrecoverable(
                "Journalpost krever saksbehandlerenhet",
            ));
        }
        if felles.dokumenter.is_empty() {
            return Err(EksekveringFeil::irrecoverable(
                "Journalpost krever minst ett dokument",
            ));
        }
        Ok(())
    }

    fn journalpost_plan(
        journalpost_id: Uuid,
        sak_key: SakKey,
        journalpost_type: JournalpostType,
        utsending: Option<Utsending>,
        dokumenter: &[lib_schemas::skuffen::dokument::Dokument],
    ) -> Result<Self, EksekveringFeil> {
        let dokument_ids: Vec<Uuid> = dokumenter.iter().map(|d| d.client_reference).collect();

        let plan = JournalpostPlan {
            journalpost_id,
            sak_key,
            journalpost_type,
            utsending,
            dokumenter: dokument_ids.clone(),
        };

        let mut steg = Vec::with_capacity(2 + dokument_ids.len());
        steg.push(Steg::OpprettJournalpost { plan });
        for dokument_id in dokument_ids {
            steg.push(Steg::LeggTilDokument {
                journalpost_id,
                dokument_id,
            });
        }
        steg.push(Steg::Journalfoer { journalpost_id });
        if journalpost_type == JournalpostType::Inngaende {
            steg.push(Steg::Avskriv { journalpost_id });
        }

        Ok(Self { steg })
    }
}
