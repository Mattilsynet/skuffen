// fn from_dto_journalpost_to_domain(dto_journalpost: Journal

use lib_schemas::skuffen::journalpost::JournalpostKey;

pub fn from_dto_journalpost_key_to_domain(
    dto_journalpost_key: JournalpostKey,
) -> Result<domain::model::journalpost::JournalpostKey, anyhow::Error> {
    match dto_journalpost_key {
        JournalpostKey::ClientReference(uuid) => {
            Ok(domain::model::journalpost::JournalpostKey::SkuffenId(uuid))
        }
        JournalpostKey::JournalpostId(journalpost_id) => {
            Ok(domain::model::journalpost::JournalpostKey::ArkivId(
                domain::model::journalpost::JournalpostId(journalpost_id.as_str().to_string()),
            ))
        }
    }
}
