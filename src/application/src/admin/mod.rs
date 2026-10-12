//! Admin: den lokale tilstanden en reparasjon må forstås ut fra, og et
//! kontrollert nytt forsøk når årsaken er rettet.
//!
//! Admin read leser bare persistert PostgreSQL-state. Den rekonstruerer ingen
//! hendelsestidslinje, leser ikke status-streamen og kaller ikke arkivet.
//!
//! Admin retry (SKU-0023) slipper stoppet arbeid inn i ordinær behandling
//! igjen. Den kaller heller ikke arkivet selv.

pub mod model;
pub mod ports;
pub mod services;

pub use model::{
    AdminCommand, AdminCommandUtfall, AdminDokument, AdminEntitetIdentitet, AdminJournalpost,
    AdminKorrespondansepart, AdminOperasjonDetaljer, AdminOperasjonEntitet,
    AdminOperasjonSammendrag, AdminSak, AdminSakFakta, AdminSakNokkel,
};
