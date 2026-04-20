use lib_schemas::skuffen::command::commands::Command;
use lib_schemas::skuffen::command::journalpost::JournalpostCommon;

use crate::eksekvering::typer::EksekveringFeil;

pub fn valider_kommando(command: &Command) -> Result<(), EksekveringFeil> {
    match command {
        Command::OpprettSak(_) => Ok(()),
        Command::AvsluttSak(_) => Ok(()),
        Command::OpprettInngåendeJournalpost(cmd) => {
            valider_felles(&cmd.felles)?;
            if cmd.avsender.trim().is_empty() {
                return Err(EksekveringFeil::irrecoverable(
                    "Inngående journalpost krever avsender",
                ));
            }
            Ok(())
        }
        Command::OpprettUtgåendeJournalpost(cmd) => {
            valider_felles(&cmd.felles)?;
            if cmd.mottaker.trim().is_empty() {
                return Err(EksekveringFeil::irrecoverable(
                    "Utgående journalpost krever mottaker",
                ));
            }
            Ok(())
        }
        Command::OpprettInterntNotatJournalpost(cmd) => {
            valider_felles(&cmd.felles)?;
            Ok(())
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
