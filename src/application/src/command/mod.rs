pub mod model;
pub use model::{
    Arkivdel, AvsluttSakCommand, Command, CommandEnvelope, Dokument, Dokumentform,
    JournalpostCommon, Korrespondansepart, MottakerId, OpprettJournalpostCommand,
    OpprettSakCommand, Parttype, Postadresse, SakKey, SettSaksansvarligCommand, Tilgjengelighet,
    Utsendingsmottaker,
};

#[cfg(test)]
pub use model::test_fixtures;

pub mod materialisering;
pub mod ports;
pub mod services;
