//! Admin read: den lokale tilstanden en reparasjon må forstås ut fra.
//!
//! Admin read leser bare persistert PostgreSQL-state. Den rekonstruerer ingen
//! hendelsestidslinje, leser ikke status-streamen og kaller ikke arkivet.

pub mod model;
pub mod ports;
pub mod services;

pub use model::{
    AdminCommand, AdminCommandUtfall, AdminDokument, AdminEntitetIdentitet, AdminJournalpost,
    AdminKorrespondansepart, AdminOperasjonDetaljer, AdminOperasjonEntitet,
    AdminOperasjonSammendrag, AdminSak, AdminSakFakta, AdminSakNokkel,
};
