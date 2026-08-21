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
use std::collections::HashMap;
use std::sync::Mutex;

/// Arkivet uten Sikri. Holder journalpoststatus i minnet, så
/// `AvventJournalfort` kan observere en faktisk overgang.
#[derive(Clone, Default)]
pub struct FakeArkivGateway {
    sak_counter: Arc<AtomicUsize>,
    journalpost_counter: Arc<AtomicI32>,
    dokument_counter: Arc<AtomicI32>,
    journalstatus: Arc<Mutex<HashMap<i32, ObservertJournalstatus>>>,
}

impl FakeArkivGateway {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lar tester styre hva neste observasjon returnerer.
    pub fn sett_journalstatus(&self, journalpost_id: i32, status: ObservertJournalstatus) {
        self.journalstatus
            .lock()
            .unwrap()
            .insert(journalpost_id, status);
    }
}

#[async_trait]
impl ArkivGateway for FakeArkivGateway {
    async fn opprett_sak(
        &self,
        _attributter: &SakAttributter,
    ) -> Result<OpprettSakResultat, anyhow::Error> {
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
    ) -> Result<OpprettJournalpostResultat, anyhow::Error> {
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
    ) -> Result<Option<i32>, anyhow::Error> {
        let seq = self.dokument_counter.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(20_000 + seq))
    }

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        status: Journalstatus,
    ) -> Result<(), anyhow::Error> {
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

    async fn avskriv_journalpost(&self, _journalpost_id: i32) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// Simulerer SvarUt (`F → E`) og RPA (`E → J`), ett steg per observasjon.
    async fn hent_journalstatus(
        &self,
        journalpost_id: i32,
    ) -> Result<ObservertJournalstatus, anyhow::Error> {
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

    async fn avslutt_sak(&self, _saksnummer: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn sett_saksansvarlig(
        &self,
        _saksnummer: &str,
        _saksbehandler_id: &str,
        _saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
