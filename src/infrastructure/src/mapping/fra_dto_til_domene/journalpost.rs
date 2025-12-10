// fn from_dto_journalpost_to_domain(dto_journalpost: Journal

use lib_schemas::skuffen::journalpost::JournalpostKey;

pub fn from_dto_journalpost_key_to_domain(
    dto_journalpost_key: JournalpostKey,
) -> domain::model::journalpost::JournalpostKey {
    match dto_journalpost_key {
        JournalpostKey::SkuffenId(uuid) => {
            domain::model::journalpost::JournalpostKey::SkuffenId(uuid)
        }
        JournalpostKey::ArkivId(journalpost_id) => {
            domain::model::journalpost::JournalpostKey::ArkivId(
                domain::model::journalpost::JournalpostId(journalpost_id.as_str().to_string()),
            )
        }
    }
}
