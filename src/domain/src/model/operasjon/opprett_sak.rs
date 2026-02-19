use crate::model::sak::{Arkivdel, Saksbehandler};
use crate::model::tilgang::Tilgang;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct OpprettSak {
    pub sakstittel: String,
    pub arkivdel: Arkivdel,
    pub saksbehandler: Saksbehandler,
    pub ordningsverdi: String,
    pub tilgang: Option<Tilgang>,
    // pub virksomhetsmappe_id: Option<String>,
}
