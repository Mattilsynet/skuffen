use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use application::command::materialisering::{
    DokumentAttributter, JournalpostAttributter, SakAttributter,
};
use application::command::ports::eksekvering_port::{
    ArkivGateway, Journalstatus, ObservertJournalstatus, OpprettJournalpostResultat,
    OpprettSakResultat,
};
use async_trait::async_trait;
use domain::eksekvering::typer::{EksekveringFeil, StatusErrorCode};
use std::collections::HashMap;
use std::sync::Mutex;

/// Miljøvariabel som lar en integrasjonstest be fake-arkivet feile.
///
/// Verdien er `irrecoverable` eller `recoverable`. Uten den oppfører faken seg
/// som før. Den leses kun når `SKUFFEN_FAKE_SIKRI=1`, som allerede er sperret
/// til local/dev/test i [`crate::bootstrap`].
pub const FAKE_SIKRI_FEIL_ENV: &str = "SKUFFEN_FAKE_SIKRI_FEIL";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Feilmodus {
    #[default]
    Ingen,
    Recoverable,
    Irrecoverable,
}

impl Feilmodus {
    fn fra_env() -> Self {
        match std::env::var(FAKE_SIKRI_FEIL_ENV).ok().as_deref() {
            Some("irrecoverable") => Feilmodus::Irrecoverable,
            Some("recoverable") => Feilmodus::Recoverable,
            _ => Feilmodus::Ingen,
        }
    }

    fn som_feil(self) -> Option<EksekveringFeil> {
        match self {
            Feilmodus::Ingen => None,
            // Speiler en ekte irrecoverable Sikri-feil: stabil kode, trygg
            // brukertekst og en klientvendt feilkode.
            Feilmodus::Irrecoverable => Some(EksekveringFeil::irrecoverable(
                "sikri_unknown_user",
                "Ugyldig saksbehandler/systembruker: brukeren finnes ikke i ePhorte.",
                StatusErrorCode::InvalidRequest,
            )),
            Feilmodus::Recoverable => Some(EksekveringFeil::recoverable(
                "sikri_upstream_unavailable",
                "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere.",
                StatusErrorCode::TemporaryUnavailable,
            )),
        }
    }
}

/// Arkivet uten Sikri. Holder journalpoststatus i minnet, så
/// `AvventJournalfort` kan observere en faktisk overgang.
#[derive(Clone, Default)]
pub struct FakeArkivGateway {
    sak_counter: Arc<AtomicUsize>,
    journalpost_counter: Arc<AtomicI32>,
    dokument_counter: Arc<AtomicI32>,
    journalstatus: Arc<Mutex<HashMap<i32, ObservertJournalstatus>>>,
    feilmodus: Feilmodus,
}

impl FakeArkivGateway {
    pub fn new() -> Self {
        Self {
            feilmodus: Feilmodus::fra_env(),
            ..Default::default()
        }
    }

    pub fn med_feilmodus(feilmodus: Feilmodus) -> Self {
        Self {
            feilmodus,
            ..Default::default()
        }
    }

    /// Lar tester styre hva neste observasjon returnerer.
    pub fn sett_journalstatus(&self, journalpost_id: i32, status: ObservertJournalstatus) {
        self.journalstatus
            .lock()
            .unwrap()
            .insert(journalpost_id, status);
    }

    /// Feiler hvert arkivkall så lenge modusen står. Vedvarende, ikke
    /// engangs: en recoverable feil skal kunne observeres over flere forsøk.
    fn sjekk_feilmodus(&self) -> Result<(), EksekveringFeil> {
        match self.feilmodus.som_feil() {
            Some(feil) => Err(feil),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl ArkivGateway for FakeArkivGateway {
    async fn opprett_sak(
        &self,
        _attributter: &SakAttributter,
    ) -> Result<OpprettSakResultat, EksekveringFeil> {
        self.sjekk_feilmodus()?;
        let seq = self.sak_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let saksnummer = format!("2026/{:06}", 900000 + seq);
        super::fake_command_state_repo::registrer_fake_sak(&saksnummer);
        Ok(OpprettSakResultat { saksnummer })
    }

    async fn opprett_journalpost(
        &self,
        _saksnummer: &str,
        _journalpost: &JournalpostAttributter,
        _hoveddokument: &DokumentAttributter,
    ) -> Result<OpprettJournalpostResultat, EksekveringFeil> {
        self.sjekk_feilmodus()?;
        let seq = self.journalpost_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let journalpost_id = 10_000 + seq;
        self.journalstatus
            .lock()
            .unwrap()
            .insert(journalpost_id, ObservertJournalstatus::Reservert);
        Ok(OpprettJournalpostResultat { journalpost_id })
    }

    async fn legg_til_vedlegg(
        &self,
        _journalpost_id: i32,
        _vedlegg: &DokumentAttributter,
    ) -> Result<Option<i32>, EksekveringFeil> {
        self.sjekk_feilmodus()?;
        let seq = self.dokument_counter.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(20_000 + seq))
    }

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        status: Journalstatus,
    ) -> Result<(), EksekveringFeil> {
        self.sjekk_feilmodus()?;
        let observert = match status {
            Journalstatus::Journalfoert => ObservertJournalstatus::Journalfoert,
            Journalstatus::Ekspedert => ObservertJournalstatus::Ekspedert,
            Journalstatus::KlarForEkspedering => ObservertJournalstatus::KlarForEkspedering,
        };
        self.journalstatus
            .lock()
            .unwrap()
            .insert(journalpost_id, observert);
        Ok(())
    }

    async fn avskriv_journalpost(&self, _journalpost_id: i32) -> Result<(), EksekveringFeil> {
        self.sjekk_feilmodus()?;
        Ok(())
    }

    /// Simulerer SvarUt (`F → E`) og RPA (`E → J`), ett steg per observasjon.
    async fn hent_journalstatus(
        &self,
        journalpost_id: i32,
    ) -> Result<ObservertJournalstatus, EksekveringFeil> {
        self.sjekk_feilmodus()?;
        let mut statuser = self.journalstatus.lock().unwrap();
        let naavaerende = statuser
            .get(&journalpost_id)
            .copied()
            .unwrap_or(ObservertJournalstatus::Annet);

        let neste = match naavaerende {
            ObservertJournalstatus::KlarForEkspedering => ObservertJournalstatus::Ekspedert,
            ObservertJournalstatus::Ekspedert => ObservertJournalstatus::Journalfoert,
            annet => annet,
        };
        statuser.insert(journalpost_id, neste);

        Ok(neste)
    }

    async fn avslutt_sak(&self, _saksnummer: &str) -> Result<(), EksekveringFeil> {
        self.sjekk_feilmodus()?;
        Ok(())
    }

    async fn sett_saksansvarlig(
        &self,
        _saksnummer: &str,
        _saksbehandler_id: &str,
        _saksbehandler_enhet: &str,
    ) -> Result<(), EksekveringFeil> {
        self.sjekk_feilmodus()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn uten_feilmodus_lykkes_arkivkallet() {
        let gateway = FakeArkivGateway::med_feilmodus(Feilmodus::Ingen);
        assert!(gateway.avslutt_sak("2026/000001").await.is_ok());
    }

    #[tokio::test]
    async fn irrecoverable_feilmodus_gir_terminal_feil() {
        let gateway = FakeArkivGateway::med_feilmodus(Feilmodus::Irrecoverable);
        let feil = gateway.avslutt_sak("2026/000001").await.unwrap_err();

        assert!(!feil.er_recoverable());
        assert_eq!(feil.kode, "sikri_unknown_user");
        assert_eq!(feil.error_code, StatusErrorCode::InvalidRequest);
    }

    #[tokio::test]
    async fn feilmodus_er_vedvarende_ikke_engangs() {
        let gateway = FakeArkivGateway::med_feilmodus(Feilmodus::Recoverable);

        for _ in 0..3 {
            let feil = gateway.avslutt_sak("2026/000001").await.unwrap_err();
            assert!(feil.er_recoverable());
        }
    }
}
