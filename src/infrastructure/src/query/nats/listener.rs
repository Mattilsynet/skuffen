use std::fmt::Debug;

use async_trait::async_trait;
use futures::StreamExt;
use lib_schemas::skuffen::query::{
    queries::{HentJournalpostQuery, HentSakQuery},
    responses::{JournalpostResponse, SakResponse},
};
use tracing::{debug, error, info};

use crate::nats::{client::NatsClient, nats_response::NatsResponse};
use crate::query::mapping::fra_domene_til_dto::{
    journalpost::from_domain_journalpost_to_dto, sak::from_domain_sak_to_dto,
};
use crate::query::mapping::fra_dto_til_domene::{
    journalpost::from_dto_journalpost_key_to_domain, sak::from_dto_sak_key_to_domain,
};

pub const HENT_SAK_SUBJECT: &str = "arkiv.request.sak.hent";
pub const HENT_JOURNALPOST_SUBJECT: &str = "arkiv.request.journalpost.hent";
pub const BRUKER_MT_ENHETER_SUBJECT: &str = "arkiv.request.bruker.mt_enheter";

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
        let domain_sak = application::query::ports::use_cases::HentSakUseCase::handle(
            self,
            from_dto_sak_key_to_domain(req.key).await?,
            false,
        )
        .await?;
        let response = from_domain_sak_to_dto(domain_sak).await?;
        Ok(response)
    }
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
    #[tracing::instrument(skip_all)]
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Lytter etter meldinger på subject '{}'", self.subject);
        let mut sub = self.client.inner().subscribe(self.subject.clone()).await?;

        while let Some(msg) = sub.next().await {
            self.process_message(msg).await;
        }

        Ok(())
    }

    #[tracing::instrument(skip_all, name = "query.handle", fields(subject = %self.subject))]
    async fn process_message(&self, msg: async_nats::Message) {
        crate::telemetry::set_parent_from_nats_headers(msg.headers.as_ref());
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
