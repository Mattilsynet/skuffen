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
/// Verdien er `<modus>` eller `<modus>@<operasjonstype>`, der modusen er
/// `irrecoverable`, `recoverable`, `uavskrevne_restanser` eller
/// `manglende_dokumentinnhold`. Uten `@` feiler hvert kall. Den leses kun når
/// `SKUFFEN_FAKE_SIKRI=1`, som allerede er sperret til local/dev/test i
/// [`crate::bootstrap`].
pub const FAKE_SIKRI_FEIL_ENV: &str = "SKUFFEN_FAKE_SIKRI_FEIL";

/// Hvilket arkivkall som utføres. Lar feilinjeksjonen treffe én operasjon i
/// en ellers vellykket sekvens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arkivkall {
    OpprettSak,
    OpprettJournalpost,
    LeggTilVedlegg,
    SettJournalpostStatus,
    Avskriv,
    HentJournalstatus,
    AvsluttSak,
    SettSaksansvarlig,
}

impl Arkivkall {
    fn fra_kode(kode: &str) -> Option<Self> {
        let kall = match kode {
            "opprett_sak" => Self::OpprettSak,
            "opprett_journalpost" => Self::OpprettJournalpost,
            "legg_til_vedlegg" => Self::LeggTilVedlegg,
            "sett_journalpost_status" => Self::SettJournalpostStatus,
            "avskriv" => Self::Avskriv,
            "hent_journalstatus" => Self::HentJournalstatus,
            "avslutt_sak" => Self::AvsluttSak,
            "sett_saksansvarlig" => Self::SettSaksansvarlig,
            _ => return None,
        };
        Some(kall)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Feilmodus {
    #[default]
    Ingen,
    Recoverable,
    Irrecoverable,
    UavskrevneRestanser,
    ManglendeDokumentinnhold,
}

/// Syntetiske Sikri-svar. De går gjennom den ekte klassifiseringen, slik at
/// runtime-testene måler reglene og ikke en håndlaget feil.
const RESTANSESVAR: &str = r#"{"errorMessage":"Det finnes 2 ikke avskrevne restanser"}"#;
const VEDLEGGSSVAR: &str =
    r#"{"errorMessage":"Vedleggslisten har dokument-filer som mangler innhold"}"#;

impl Feilmodus {
    fn fra_kode(kode: &str) -> Self {
        match kode {
            "irrecoverable" => Feilmodus::Irrecoverable,
            "recoverable" => Feilmodus::Recoverable,
            "uavskrevne_restanser" => Feilmodus::UavskrevneRestanser,
            "manglende_dokumentinnhold" => Feilmodus::ManglendeDokumentinnhold,
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
            Feilmodus::UavskrevneRestanser => Some(sikri_svar(RESTANSESVAR)),
            Feilmodus::ManglendeDokumentinnhold => Some(sikri_svar(VEDLEGGSSVAR)),
        }
    }
}

fn sikri_svar(body: &str) -> EksekveringFeil {
    super::sikri_arkiv_gateway::fra_sikri(sikri_client::SikriFeil::fra_http(
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        Some(body),
    ))
}

/// Hvilken feil som injiseres, og hvilket kall den treffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Feilinjeksjon {
    modus: Feilmodus,
    maal: Option<Arkivkall>,
}

impl Feilinjeksjon {
    fn fra_env() -> Self {
        match std::env::var(FAKE_SIKRI_FEIL_ENV) {
            Ok(verdi) => Self::fra_kode(&verdi),
            Err(_) => Self::default(),
        }
    }

    fn fra_kode(verdi: &str) -> Self {
        let (modus, maal) = match verdi.split_once('@') {
            Some((modus, maal)) => (modus, Arkivkall::fra_kode(maal)),
            None => (verdi, None),
        };
        Self {
            modus: Feilmodus::fra_kode(modus),
            maal,
        }
    }

    fn feil_for(self, kall: Arkivkall) -> Option<EksekveringFeil> {
        if self.maal.is_some_and(|maal| maal != kall) {
            return None;
        }
        self.modus.som_feil()
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
    feilinjeksjon: Feilinjeksjon,
}

impl FakeArkivGateway {
    pub fn new() -> Self {
        Self {
            feilinjeksjon: Feilinjeksjon::fra_env(),
            ..Default::default()
        }
    }

    pub fn med_feilmodus(modus: Feilmodus) -> Self {
        Self {
            feilinjeksjon: Feilinjeksjon { modus, maal: None },
            ..Default::default()
        }
    }

    pub fn med_feilinjeksjon(verdi: &str) -> Self {
        Self {
            feilinjeksjon: Feilinjeksjon::fra_kode(verdi),
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

    /// Feiler så lenge modusen står. Vedvarende, ikke engangs: en recoverable
    /// feil skal kunne observeres over flere forsøk.
    fn sjekk_feilmodus(&self, kall: Arkivkall) -> Result<(), EksekveringFeil> {
        match self.feilinjeksjon.feil_for(kall) {
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
        self.sjekk_feilmodus(Arkivkall::OpprettSak)?;
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
        self.sjekk_feilmodus(Arkivkall::OpprettJournalpost)?;
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
        self.sjekk_feilmodus(Arkivkall::LeggTilVedlegg)?;
        let seq = self.dokument_counter.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(20_000 + seq))
    }

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        status: Journalstatus,
    ) -> Result<(), EksekveringFeil> {
        self.sjekk_feilmodus(Arkivkall::SettJournalpostStatus)?;
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

    async fn avskriv_journalpost(
        &self,
        _journalpost_id: i32,
        _kildesystem: Option<&str>,
        _merknad: Option<&str>,
    ) -> Result<(), EksekveringFeil> {
        self.sjekk_feilmodus(Arkivkall::Avskriv)?;
        Ok(())
    }

    /// Simulerer SvarUt (`F → E`) og RPA (`E → J`), ett steg per observasjon.
    async fn hent_journalstatus(
        &self,
        journalpost_id: i32,
    ) -> Result<ObservertJournalstatus, EksekveringFeil> {
        self.sjekk_feilmodus(Arkivkall::HentJournalstatus)?;
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
        self.sjekk_feilmodus(Arkivkall::AvsluttSak)?;
        Ok(())
    }

    async fn sett_saksansvarlig(
        &self,
        _saksnummer: &str,
        _saksbehandler_id: &str,
        _saksbehandler_enhet: &str,
    ) -> Result<(), EksekveringFeil> {
        self.sjekk_feilmodus(Arkivkall::SettSaksansvarlig)?;
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

    #[tokio::test]
    async fn feilinjeksjon_kan_begrenses_til_en_operasjon() {
        let gateway = FakeArkivGateway::med_feilinjeksjon("uavskrevne_restanser@avslutt_sak");

        let feil = gateway.avslutt_sak("2026/000001").await.unwrap_err();
        assert!(!feil.er_recoverable());
        assert_eq!(feil.kode, "sikri_unresolved_journalposter");
        assert_eq!(feil.error_code, StatusErrorCode::PrerequisitePending);
        assert_eq!(
            feil.melding,
            "Saken har journalposter som ikke er avskrevet (restanser) og kan ikke avsluttes."
        );

        assert!(
            gateway
                .sett_saksansvarlig("2026/000001", "Z12345", "MT-1")
                .await
                .is_ok(),
            "andre kall skal lykkes som før"
        );
    }

    #[tokio::test]
    async fn vedleggsfeil_treffer_bare_vedleggskallet() {
        let gateway =
            FakeArkivGateway::med_feilinjeksjon("manglende_dokumentinnhold@legg_til_vedlegg");
        let vedlegg = DokumentAttributter {
            tittel: "Vedlegg".to_string(),
            rekkefolge: 1,
            kilde: application::command::materialisering::Dokumentkilde::Bytes {
                dokument_referanse: uuid::Uuid::from_u128(2),
                filtype: "PDF".to_string(),
            },
        };

        let feil = gateway.legg_til_vedlegg(1, &vedlegg).await.unwrap_err();
        assert!(!feil.er_recoverable());
        assert_eq!(feil.kode, "sikri_missing_document_content");
        assert_eq!(feil.error_code, StatusErrorCode::InvalidRequest);

        assert!(gateway.avslutt_sak("2026/000001").await.is_ok());
    }
}
