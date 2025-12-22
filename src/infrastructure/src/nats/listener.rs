use std::fmt::Debug;

use async_trait::async_trait;
use futures::StreamExt;
use lib_schemas::skuffen::{
    journalpost::JournalpostResponse,
    query::queries::{HentJournalpostQuery, HentSakQuery},
    sak::SakResponse,
};
use tracing::{error, info};

use crate::{
    mapping::{
        fra_domene_til_dto::{
            journalpost::from_domain_journalpost_to_dto, sak::from_domain_sak_to_dto,
        },
        fra_dto_til_domene::{
            journalpost::from_dto_journalpost_key_to_domain, sak::from_dto_sak_key_to_domain,
        },
    },
    nats::{client::NatsClient, nats_response::NatsResponse},
};

#[async_trait]
pub trait UseCase<Request, Response> {
    async fn handle(&self, req: Request) -> Result<Response, anyhow::Error>;
}

#[async_trait]
impl<T> UseCase<HentSakQuery, SakResponse> for T
where
    T: application::ports::use_cases::HentSakUseCase + Send + Sync,
{
    async fn handle(&self, req: HentSakQuery) -> Result<SakResponse, anyhow::Error> {
        let domain_sak = application::ports::use_cases::HentSakUseCase::handle(
            self,
            from_dto_sak_key_to_domain(req.key).await?,
            req.inkluder_journalposter,
        )
        .await?;
        let response = from_domain_sak_to_dto(domain_sak).await?;
        Ok(response)
    }
}

#[async_trait]
impl<T> UseCase<HentJournalpostQuery, JournalpostResponse> for T
where
    T: application::ports::use_cases::HentJournalpostUseCase + Send + Sync,
{
    async fn handle(
        &self,
        req: HentJournalpostQuery,
    ) -> Result<JournalpostResponse, anyhow::Error> {
        let domain_journalpost = application::ports::use_cases::HentJournalpostUseCase::handle(
            self,
            from_dto_journalpost_key_to_domain(req.key),
        )
        .await?;
        let dto_journalpost = from_domain_journalpost_to_dto(domain_journalpost)?;
        Ok(dto_journalpost)
    }
}

#[derive(Debug)]
pub struct NatsReplier<U, Req, Res> {
    client: NatsClient,
    subject: String,
    use_case: U,
    _marker: std::marker::PhantomData<(Req, Res)>,
}

impl<U, Req, Res> NatsReplier<U, Req, Res> {
    pub fn new(client: NatsClient, subject: impl Into<String>, use_case: U) -> Self {
        Self {
            client,
            subject: subject.into(),
            use_case,
            _marker: std::marker::PhantomData,
        }
    }
}
impl<U, Req, Res> NatsReplier<U, Req, Res>
where
    U: UseCase<Req, Res> + Send + Sync + Debug,
    Req: serde::de::DeserializeOwned + Send + Debug,
    Res: serde::Serialize + Send + Debug,
{
    #[tracing::instrument()]
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Lytter etter meldinger på subject '{}'", self.subject);
        let mut sub = self.client.inner().subscribe(self.subject.clone()).await?;

        while let Some(msg) = sub.next().await {
            info!("Mottok et query på subject {}", self.subject);

            let reply_subject = match msg.reply {
                Some(r) => r,
                None => {
                    error!("NATS request has no reply subject. Ignoring message.");
                    continue;
                }
            };

            let req: Req = match serde_json::from_slice(&msg.payload) {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to deserialize request payload: {e}");
                    // Always reply with JSON error
                    let err = NatsResponse::<Res>::Error {
                        message: format!("Bad request: {e}"),
                    };
                    let bytes = serde_json::to_vec(&err)?;
                    self.client
                        .inner()
                        .publish(reply_subject, bytes.into())
                        .await?;
                    continue;
                }
            };

            let nats_response: NatsResponse<Res> = match self.use_case.handle(req).await {
                Ok(payload) => NatsResponse::Ok(payload),
                Err(e) => {
                    error!("Use case returned error: {e:?}");
                    NatsResponse::Error {
                        message: e.to_string(),
                    }
                }
            };

            let bytes = serde_json::to_vec(&nats_response)?;

            if let Err(e) = self
                .client
                .inner()
                .publish(reply_subject, bytes.into())
                .await
            {
                error!("Failed to publish reply: {:?}", e);
            } else {
                tracing::info!("Successfully replied with JSON NatsResponse");
            }
        }

        Ok(())
    }
}
