//! Feilmeldingene `arkiv.arkiver` svarer klienten med.
//!
//! Meldingene skal kunne leses av et menneske og si hva som må rettes.

use domain::command::batch::{Batchfeil, Entitetsplass, Forekomst, Posisjon};
use lib_schemas::skuffen::command::commands::Command;
use uuid::Uuid;

const AVVIST: &str = "Batchen ble avvist, og ingen kommandoer er tatt imot.";

pub const TOM_BATCH: &str =
    "Batchen ble avvist fordi den er tom. Send en liste med minst én kommando.";

pub const INTERN_FEIL: &str = "Batchen kunne ikke tas imot på grunn av en intern feil i Skuffen. \
Kommandoer før feilen kan allerede være tatt imot. Send hele batchen på nytt med de samme \
command_id-ene; det som allerede er tatt imot, blir ikke behandlet to ganger.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mediafeil {
    pub posisjon: Posisjon,
    /// Posisjon i journalpostens dokumentliste, 0-basert.
    pub dokument: usize,
    pub art: MediafeilArt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediafeilArt {
    DokumentIkkeLastetOpp { dokument_referanse: Uuid },
    MalIkkeLastetOpp { mal_referanse: Uuid },
    DuplikateMalfelter,
}

/// Navnet kommandoen har i JSON-en klienten sendte.
pub fn kommandonavn(command: &Command) -> &'static str {
    match command {
        Command::OpprettSak(_) => "OpprettSak",
        Command::OpprettInngåendeJournalpost(_) => "OpprettInngåendeJournalpost",
        Command::OpprettUtgåendeJournalpost(_) => "OpprettUtgåendeJournalpost",
        Command::OpprettUtgåendeJournalpostMedUtsending(_) => {
            "OpprettUtgåendeJournalpostMedUtsending"
        }
        Command::OpprettInterntNotatJournalpost(_) => "OpprettInterntNotatJournalpost",
        Command::AvsluttSak(_) => "AvsluttSak",
        Command::SettSaksansvarlig(_) => "SettSaksansvarlig",
    }
}

pub fn ugyldig_payload(feil: &serde_json::Error) -> String {
    format!("{AVVIST} Innholdet kunne ikke leses som en kommandobatch: {feil}.")
}

pub fn ugyldig_batch(feil: &[Batchfeil], navn: &[&str]) -> String {
    liste(feil.iter().map(|feil| beskriv_batchfeil(feil, navn)))
}

pub fn manglende_media(feil: &[Mediafeil], navn: &[&str]) -> String {
    liste(feil.iter().map(|feil| {
        let sted = format!(
            "Dokument {} i {}",
            feil.dokument + 1,
            kommando(feil.posisjon, navn)
        );
        match feil.art {
            MediafeilArt::DokumentIkkeLastetOpp { dokument_referanse } => format!(
                "{sted} viser til dokument_referanse {dokument_referanse}, som ikke er lastet opp. \
                 Last opp filen via arkiv.arkiver.media og vent på bekreftet lagring før batchen sendes."
            ),
            MediafeilArt::MalIkkeLastetOpp { mal_referanse } => format!(
                "{sted} viser til mal_referanse {mal_referanse}, som ikke er lastet opp. \
                 Last opp HTML-malen via arkiv.arkiver.media og vent på bekreftet lagring før batchen sendes."
            ),
            MediafeilArt::DuplikateMalfelter => format!(
                "{sted} har samme felt flere ganger i HtmlTemplate.felter. \
                 Hvert felt kan bare oppgis én gang."
            ),
        }
    }))
}

fn liste(linjer: impl Iterator<Item = String>) -> String {
    let linjer: Vec<String> = linjer.collect();
    let mut melding = format!("{AVVIST} {} feil:", linjer.len());
    for linje in linjer {
        melding.push_str("\n- ");
        melding.push_str(&linje);
    }
    melding
}

fn beskriv_batchfeil(feil: &Batchfeil, navn: &[&str]) -> String {
    match feil {
        Batchfeil::CommandIdGjentatt {
            command_id,
            posisjoner,
        } => format!(
            "command_id {command_id} er brukt av {}. Hver kommando må ha sin egen command_id.",
            oppramsing(posisjoner.iter().map(|posisjon| kommando(*posisjon, navn)))
        ),
        Batchfeil::CommandIdBruktAvAnnenKommandotype {
            posisjon,
            command_id,
        } => format!(
            "command_id {command_id} i {} er allerede tatt imot for en annen kommandotype. \
             En command_id hører til én kommando; lag en ny command_id for denne.",
            kommando(*posisjon, navn)
        ),
        Batchfeil::ClientReferenceGjentatt {
            client_reference,
            forekomster,
        } => format!(
            "client_reference {client_reference} brukes for {}. \
             Hver sak, journalpost og hvert dokument må ha sin egen client_reference.",
            oppramsing(
                forekomster
                    .iter()
                    .map(|forekomst| entitet_i_kommando(*forekomst, navn))
            )
        ),
        Batchfeil::ClientReferenceFinnesFraFoer {
            forekomst,
            client_reference,
        } => format!(
            "client_reference {client_reference} for {} finnes allerede i Skuffen. \
             En ny sak, journalpost eller et nytt dokument må ha en ny client_reference. \
             Skal en tidligere kommando sendes på nytt, må den ha samme command_id som første gang.",
            entitet_i_kommando(*forekomst, navn)
        ),
        Batchfeil::SakReferanseUkjent {
            posisjon,
            client_reference,
        } => format!(
            "sak_key i {} viser til client_reference {client_reference}, som verken opprettes \
             tidligere i batchen eller finnes i Skuffen. Opprett saken med OpprettSak tidligere \
             i batchen, eller bruk sakens arkivId.",
            kommando(*posisjon, navn)
        ),
    }
}

fn kommando(posisjon: Posisjon, navn: &[&str]) -> String {
    match navn.get(posisjon) {
        Some(navn) => format!("kommando {} ({navn})", posisjon + 1),
        None => format!("kommando {}", posisjon + 1),
    }
}

fn entitet_i_kommando(forekomst: Forekomst, navn: &[&str]) -> String {
    let entitet = match forekomst.entitet {
        Entitetsplass::Sak => "saken".to_string(),
        Entitetsplass::Journalpost => "journalposten".to_string(),
        Entitetsplass::Dokument(nummer) => format!("dokument {}", nummer + 1),
    };
    format!("{entitet} i {}", kommando(forekomst.posisjon, navn))
}

fn oppramsing(deler: impl Iterator<Item = String>) -> String {
    let deler: Vec<String> = deler.collect();
    match deler.as_slice() {
        [] => String::new(),
        [en] => en.clone(),
        [forste @ .., siste] => format!("{} og {siste}", forste.join(", ")),
    }
}

#[cfg(test)]
mod tests;
