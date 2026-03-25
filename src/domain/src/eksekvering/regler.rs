use crate::eksekvering::plan::JournalpostType;
use crate::eksekvering::typer::EksekveringFeil;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SakRuleState {
    pub avsluttet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalpostRuleState {
    pub journalpost_type: JournalpostType,
    pub journalfoert: bool,
    pub avskrevet: bool,
    pub ekspedert: bool,
    pub har_feilede_dokumenter: bool,
    pub med_utsending: bool,
    pub journalpostnummer: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalpostStatusTransition {
    pub ny_status: &'static str,
    pub journalfoert: bool,
    pub ekspedert: bool,
}

pub fn kan_opprette_journalpost_pa_sak(sak: &SakRuleState) -> Result<(), EksekveringFeil> {
    if sak.avsluttet {
        return Err(EksekveringFeil::irrecoverable(
            "Kan ikke opprette journalpost pa avsluttet sak",
        ));
    }

    Ok(())
}

pub fn kan_journalfoere_journalpost(
    journalpost: &JournalpostRuleState,
) -> Result<(), EksekveringFeil> {
    if journalpost.har_feilede_dokumenter {
        return Err(EksekveringFeil::blocked(
            "Kan ikke journalfore journalpost: ett eller flere dokumenter har feilet",
        ));
    }

    Ok(())
}

pub fn kan_avskrive_journalpost(journalpost: &JournalpostRuleState) -> Result<(), EksekveringFeil> {
    if !journalpost.journalfoert {
        return Err(EksekveringFeil::blocked(
            "Kan ikke avskrive journalpost: journalpost er ikke journalfort",
        ));
    }

    Ok(())
}

pub fn kan_avslutte_sak(journalposter: &[JournalpostRuleState]) -> Result<(), EksekveringFeil> {
    for journalpost in journalposter {
        if journalpost.har_feilede_dokumenter {
            return Err(EksekveringFeil::blocked(
                "Kan ikke avslutte sak: minst ett dokument pa en journalpost har feilet",
            ));
        }

        match journalpost.journalpost_type {
            JournalpostType::Inngaende => {
                if !journalpost.journalfoert || !journalpost.avskrevet {
                    return Err(EksekveringFeil::blocked(describe_incomplete_journalpost(
                        journalpost,
                    )));
                }
            }
            JournalpostType::Utgaaende | JournalpostType::InterntNotat => {
                if !journalpost.journalfoert {
                    return Err(EksekveringFeil::blocked(describe_incomplete_journalpost(
                        journalpost,
                    )));
                }
            }
        }
    }

    Ok(())
}

pub fn neste_journalpost_status_ved_journalfoering(
    journalpost: &JournalpostRuleState,
) -> JournalpostStatusTransition {
    match journalpost.journalpost_type {
        JournalpostType::Utgaaende => {
            if journalpost.med_utsending {
                JournalpostStatusTransition {
                    ny_status: "F",
                    journalfoert: false,
                    ekspedert: false,
                }
            } else {
                JournalpostStatusTransition {
                    ny_status: "J",
                    journalfoert: true,
                    ekspedert: journalpost.ekspedert,
                }
            }
        }
        JournalpostType::Inngaende | JournalpostType::InterntNotat => JournalpostStatusTransition {
            ny_status: "J",
            journalfoert: true,
            ekspedert: journalpost.ekspedert,
        },
    }
}

pub fn describe_incomplete_journalpost(journalpost: &JournalpostRuleState) -> String {
    let mut mangler: Vec<&str> = Vec::new();
    if !journalpost.journalfoert {
        mangler.push("journalfort");
    }
    if !journalpost.avskrevet {
        mangler.push("avskrevet");
    }

    let journalpostnavn = match journalpost.journalpost_type {
        JournalpostType::Inngaende => "inngaende journalpost",
        JournalpostType::Utgaaende => "utgaende journalpost",
        JournalpostType::InterntNotat => "internt notat",
    };

    let krav = if mangler.is_empty() {
        "ukjent krav mangler".to_string()
    } else {
        mangler.join(" og ")
    };

    match journalpost.journalpostnummer {
        Some(journalpostnummer) => format!(
            "Kan ikke avslutte sak: {journalpostnavn} {journalpostnummer} er ikke komplett; mangler {krav}"
        ),
        None => {
            format!("Kan ikke avslutte sak: {journalpostnavn} er ikke komplett; mangler {krav}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avslutte_sak_requires_avskriving_for_inngaende() {
        let result = kan_avslutte_sak(&[JournalpostRuleState {
            journalpost_type: JournalpostType::Inngaende,
            journalfoert: true,
            avskrevet: false,
            ekspedert: false,
            har_feilede_dokumenter: false,
            med_utsending: false,
            journalpostnummer: Some(123),
        }]);

        assert!(matches!(result, Err(EksekveringFeil { .. })));
        assert_eq!(
            result.unwrap_err().melding,
            "Kan ikke avslutte sak: inngaende journalpost 123 er ikke komplett; mangler avskrevet"
        );
    }

    #[test]
    fn utgaaende_med_utsending_stays_ready_for_dispatch() {
        let transition = neste_journalpost_status_ved_journalfoering(&JournalpostRuleState {
            journalpost_type: JournalpostType::Utgaaende,
            journalfoert: false,
            avskrevet: false,
            ekspedert: false,
            har_feilede_dokumenter: false,
            med_utsending: true,
            journalpostnummer: Some(42),
        });

        assert_eq!(transition.ny_status, "F");
        assert!(!transition.journalfoert);
        assert!(!transition.ekspedert);
    }
}
