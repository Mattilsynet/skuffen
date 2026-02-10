pub mod opprett_sak;

pub use opprett_sak::OpprettSak;

pub enum Operasjon {
    OpprettSak(OpprettSak),
}
