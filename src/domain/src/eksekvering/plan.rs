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
        sak_key: SakKey,
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
            Command::AvsluttSak(cmd) => Ok(Self {
                steg: vec![Steg::AvsluttSak {
                    sak_key: cmd.sak_key.clone(),
                }],
            }),
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
        for dokument_id in dokument_ids.into_iter().skip(1) {
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

#[cfg(test)]
mod tests {
    use super::{EksekveringsPlan, Steg};
    use lib_schemas::skuffen::command::commands::Command;
    use lib_schemas::skuffen::command::journalpost::{
        JournalpostCommon, OpprettInterntNotatJournalpost,
    };
    use lib_schemas::skuffen::dokument::Dokument;
    use lib_schemas::skuffen::query::queries::SakKey;
    use uuid::Uuid;

    #[test]
    fn single_document_journalpost_only_creates_journalpost_then_journalfoerer() {
        let document = sample_document("Hoveddokument", "PDF");
        let command = sample_internt_notat_command(vec![document.clone()]);

        let plan = EksekveringsPlan::fra_command(&command).expect("plan should be created");

        assert_eq!(plan.steg.len(), 2);
        assert!(matches!(plan.steg[0], Steg::OpprettJournalpost { .. }));
        assert!(matches!(plan.steg[1], Steg::Journalfoer { .. }));
    }

    #[test]
    fn multi_document_journalpost_only_adds_documents_after_the_first_as_attachments() {
        let hoveddokument = sample_document("Rapport", "PDF");
        let vedlegg_ett = sample_document("Bilde 1", "PNG");
        let vedlegg_to = sample_document("Bilde 2", "PNG");
        let command = sample_internt_notat_command(vec![
            hoveddokument.clone(),
            vedlegg_ett.clone(),
            vedlegg_to.clone(),
        ]);

        let plan = EksekveringsPlan::fra_command(&command).expect("plan should be created");

        assert_eq!(plan.steg.len(), 4);
        assert!(matches!(plan.steg[0], Steg::OpprettJournalpost { .. }));
        assert!(matches!(
            plan.steg[1],
            Steg::LeggTilDokument { dokument_id, .. } if dokument_id == vedlegg_ett.client_reference
        ));
        assert!(matches!(
            plan.steg[2],
            Steg::LeggTilDokument { dokument_id, .. } if dokument_id == vedlegg_to.client_reference
        ));
        assert!(matches!(plan.steg[3], Steg::Journalfoer { .. }));
    }

    fn sample_internt_notat_command(dokumenter: Vec<Dokument>) -> Command {
        Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
            felles: JournalpostCommon {
                client_reference: Uuid::new_v4(),
                tittel: "Internt notat".to_string(),
                dokument_dato: "2025-01-01".to_string(),
                saksbehandler: "Z12345".to_string(),
                saksbehandler_enhet: "1234".to_string(),
                tilgang: None,
                dokumenter,
                sak_key: SakKey::ClientReference(Uuid::new_v4()),
                kildesystem: None,
            },
        })
    }

    fn sample_document(tittel: &str, filtype: &str) -> Dokument {
        Dokument {
            client_reference: Uuid::new_v4(),
            tittel: tittel.to_string(),
            filtype: filtype.to_string(),
            dokument_referanse: Uuid::new_v4(),
        }
    }
}
