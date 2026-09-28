use std::fmt::Debug;

use async_trait::async_trait;
use futures::StreamExt;
use lib_schemas::skuffen::query::{
    queries::{HentJournalpostQuery, HentSakMedJournalposterQuery, HentSakQuery},
    responses::{JournalpostResponse, SakResponse},
};
use tracing::{Instrument, debug, error, info};

use crate::nats::{client::NatsClient, nats_response::NatsResponse};
use crate::query::mapping::fra_domene_til_dto::{
    journalpost::from_domain_journalpost_to_dto, sak::from_domain_sak_to_dto,
};
use crate::query::mapping::fra_dto_til_domene::{
    journalpost::from_dto_journalpost_key_to_domain, sak::from_dto_sak_key_to_domain,
};

pub const HENT_SAK_SUBJECT: &str = "arkiv.request.sak.hent";
pub const HENT_SAK_MED_JOURNALPOSTER_SUBJECT: &str = "arkiv.request.sak.med_journalposter";
pub const HENT_JOURNALPOST_SUBJECT: &str = "arkiv.request.journalpost.hent";
pub const BRUKER_MT_ENHETER_SUBJECT: &str = "arkiv.request.bruker.mt_enheter";

const MAKS_SVAR_BYTES: usize = 8 * 1024 * 1024;
const RESPONSE_TOO_LARGE: &str = "Response too large";

#[async_trait]
pub trait UseCase<Request, Response> {
    async fn handle(&self, req: Request) -> Result<Response, anyhow::Error>;
}

#[derive(Debug, serde::Deserialize)]
pub struct BrukerMtEnheterRequest {}

#[derive(Debug, serde::Serialize)]
/// Tom payload-type for bruker/enhet-queryen; stubben returnerer foreløpig bare `NatsResponse::Error`.
pub struct BrukerMtEnheterResponse {}

#[derive(Debug, thiserror::Error)]
enum QueryHandlerError {
    #[error("Not implemented")]
    NotImplemented,
}

impl QueryHandlerError {
    fn nats_error_message(error: &anyhow::Error) -> &'static str {
        match error.downcast_ref::<Self>() {
            Some(Self::NotImplemented) => "Not implemented",
            None => "Internal error",
        }
    }
}

#[derive(Debug)]
pub struct BrukerMtEnheterNotImplementedUseCase;

#[async_trait]
impl UseCase<BrukerMtEnheterRequest, BrukerMtEnheterResponse>
    for BrukerMtEnheterNotImplementedUseCase
{
    async fn handle(
        &self,
        _req: BrukerMtEnheterRequest,
    ) -> Result<BrukerMtEnheterResponse, anyhow::Error> {
        Err(QueryHandlerError::NotImplemented.into())
    }
}

#[async_trait]
impl<T> UseCase<HentSakQuery, SakResponse> for T
where
    T: application::query::ports::use_cases::HentSakUseCase + Send + Sync,
{
    async fn handle(&self, req: HentSakQuery) -> Result<SakResponse, anyhow::Error> {
        let mut response = hent_sak(self, req.key, false).await?;
        response.journalposter.get_or_insert_with(Vec::new);
        Ok(response)
    }
}

#[async_trait]
impl<T> UseCase<HentSakMedJournalposterQuery, SakResponse> for T
where
    T: application::query::ports::use_cases::HentSakUseCase + Send + Sync,
{
    async fn handle(
        &self,
        req: HentSakMedJournalposterQuery,
    ) -> Result<SakResponse, anyhow::Error> {
        hent_sak(self, req.key, true).await
    }
}

async fn hent_sak<T>(
    use_case: &T,
    key: lib_schemas::skuffen::query::queries::SakKey,
    inkluder_journalposter: bool,
) -> Result<SakResponse, anyhow::Error>
where
    T: application::query::ports::use_cases::HentSakUseCase + Send + Sync,
{
    let domain_sak = application::query::ports::use_cases::HentSakUseCase::handle(
        use_case,
        from_dto_sak_key_to_domain(key)?,
        inkluder_journalposter,
    )
    .await?;
    from_domain_sak_to_dto(domain_sak)
}

#[async_trait]
impl<T> UseCase<HentJournalpostQuery, JournalpostResponse> for T
where
    T: application::query::ports::use_cases::HentJournalpostUseCase + Send + Sync,
{
    async fn handle(
        &self,
        req: HentJournalpostQuery,
    ) -> Result<JournalpostResponse, anyhow::Error> {
        let domain_journalpost =
            application::query::ports::use_cases::HentJournalpostUseCase::handle(
                self,
                from_dto_journalpost_key_to_domain(req.key)?,
            )
            .await?;
        let dto_journalpost = from_domain_journalpost_to_dto(domain_journalpost)?;
        Ok(dto_journalpost)
    }
}

// #[derive(Debug)] // removed derive
pub struct NatsReplier<Req, Res> {
    client: NatsClient,
    subject: String,
    use_case: Box<dyn UseCase<Req, Res> + Send + Sync>,
    _marker: std::marker::PhantomData<(Req, Res)>,
}

impl<Req, Res> Debug for NatsReplier<Req, Res> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NatsReplier")
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

impl<Req, Res> NatsReplier<Req, Res> {
    pub fn new(
        client: NatsClient,
        subject: impl Into<String>,
        use_case: Box<dyn UseCase<Req, Res> + Send + Sync>,
    ) -> Self {
        Self {
            client,
            subject: subject.into(),
            use_case,
            _marker: std::marker::PhantomData,
        }
    }
}
impl<Req, Res> NatsReplier<Req, Res>
where
    Req: serde::de::DeserializeOwned + Send + Debug,
    Res: serde::Serialize + Send + Debug,
{
    /// En avsluttet subscription returnerer `Err`, slik at `try_join!` ikke
    /// venter for alltid og supervisoren får kontroll (SKU-0021 R7).
    #[tracing::instrument(skip_all)]
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Lytter etter meldinger på subject '{}'", self.subject);
        // Uten queue group svarer alle instanser på samme forespørsel. Det
        // skjer ved utrullingsoverlapp, når to revisjoner lever samtidig.
        let queue_group = format!("skuffen-query-{}", self.subject);
        let mut sub = self
            .client
            .inner()
            .queue_subscribe(self.subject.clone(), queue_group)
            .await?;

        while let Some(msg) = sub.next().await {
            self.process_message(msg).await;
        }

        Err(anyhow::anyhow!(
            "subscription on {} ended unexpectedly",
            self.subject
        ))
    }

    async fn process_message(&self, msg: async_nats::Message) {
        let span = tracing::info_span!("query.handle", subject = %self.subject);
        crate::telemetry::set_parent_on_span_from_nats_headers(&span, msg.headers.as_ref());
        self.handle_message(msg).instrument(span).await
    }

    async fn handle_message(&self, msg: async_nats::Message) {
        debug!("Mottok et query på subject {}", self.subject);

        let reply_subject = match msg.reply {
            Some(r) => r,
            None => {
                error!("NATS request has no reply subject. Ignoring message.");
                return;
            }
        };

        let req: Req = match serde_json::from_slice(&msg.payload) {
            Ok(r) => r,
            Err(_) => {
                error!(
                    payload_size = msg.payload.len(),
                    "Failed to deserialize request payload"
                );
                let err = NatsResponse::<Res>::Error {
                    message: "Invalid request format".to_string(),
                };
                if let Ok(bytes) = serde_json::to_vec(&err) {
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, bytes.into())
                        .await;
                }
                return;
            }
        };

        let nats_response: NatsResponse<Res> = match self.use_case.handle(req).await {
            Ok(payload) => NatsResponse::Ok(payload),
            Err(e) => {
                error!(error = %e, "Use case returned error");
                NatsResponse::Error {
                    message: QueryHandlerError::nats_error_message(&e).to_string(),
                }
            }
        };

        let bytes = match serde_json::to_vec(&nats_response) {
            Ok(b) if b.len() > MAKS_SVAR_BYTES => {
                error!(bytes = b.len(), "Query response exceeds size limit");
                match serde_json::to_vec(&NatsResponse::<Res>::Error {
                    message: RESPONSE_TOO_LARGE.to_string(),
                }) {
                    Ok(b) => b,
                    Err(e) => {
                        error!(error = %e, "Failed to serialize response");
                        return;
                    }
                }
            }
            Ok(b) => b,
            Err(e) => {
                error!(error = %e, "Failed to serialize response");
                return;
            }
        };

        if let Err(e) = self
            .client
            .inner()
            .publish(reply_subject, bytes.into())
            .await
        {
            error!("Failed to publish reply: {:?}", e);
        } else {
            debug!("Successfully replied with JSON NatsResponse");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::model::sak::{Ordningsverdi, Sak, SakKey, Saksnummer, Saksstatus, Sakstittel};
    use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingHentSak {
        kall: Mutex<Vec<(SakKey, bool)>>,
    }

    #[async_trait]
    impl application::query::ports::use_cases::HentSakUseCase for RecordingHentSak {
        async fn handle(
            &self,
            req: SakKey,
            inkluder_journalposter: bool,
        ) -> Result<Sak, anyhow::Error> {
            self.kall
                .lock()
                .unwrap()
                .push((req, inkluder_journalposter));
            Ok(Sak {
                client_reference: None,
                sakstittel: Sakstittel("Sak".to_string()),
                saksbehandler: "Z00001".to_string(),
                saksstatus: Saksstatus::UnderBehandling,
                tilgang: None,
                saksnummer: Saksnummer::new("2026/1")?,
                kildesystem: "SKUFFEN".to_string(),
                lukket: false,
                journalposter: None,
                ordningsverdi: Ordningsverdi::new("430".to_string())?,
            })
        }
    }

    fn arkiv_key() -> DtoSakKey {
        DtoSakKey::ArkivId(lib_schemas::skuffen::sak::Saksnummer::new("2026/1").unwrap())
    }

    #[tokio::test]
    async fn sak_hent_ber_ikke_om_journalposter_og_beholder_tom_liste() {
        let use_case = RecordingHentSak::default();

        let response = UseCase::<HentSakQuery, SakResponse>::handle(
            &use_case,
            HentSakQuery { key: arkiv_key() },
        )
        .await
        .unwrap();

        assert_eq!(response.journalposter, Some(vec![]));
        assert_eq!(
            *use_case.kall.lock().unwrap(),
            vec![(SakKey::ArkivId(Saksnummer::new("2026/1").unwrap()), false)]
        );
    }

    #[tokio::test]
    async fn sak_med_journalposter_ber_alltid_om_journalposter() {
        let use_case = RecordingHentSak::default();

        let response = UseCase::<HentSakMedJournalposterQuery, SakResponse>::handle(
            &use_case,
            HentSakMedJournalposterQuery { key: arkiv_key() },
        )
        .await
        .unwrap();

        assert_eq!(response.journalposter, None);
        assert!(use_case.kall.lock().unwrap()[0].1);
    }
}
