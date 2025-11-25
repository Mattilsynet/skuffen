use async_trait::async_trait;
use futures::StreamExt;
use lib_schemas::arkiv::v2::{
    journalpost::{HentJournalpostRequest, JournalpostResponse},
    sak::{HentSakRequest, SakResponse},
};
use tracing::error;

use crate::nats::{client::NatsClient, nats_response::NatsResponse};

#[async_trait]
pub trait UseCase<Request, Response> {
    async fn handle(&self, req: Request) -> Result<Response, anyhow::Error>;
}

#[async_trait]
impl<T> UseCase<HentSakRequest, SakResponse> for T
where
    T: application::ports::use_cases::HentSakUseCase + Send + Sync,
{
    async fn handle(&self, req: HentSakRequest) -> Result<SakResponse, anyhow::Error> {
        application::ports::use_cases::HentSakUseCase::handle(self, req).await
    }
}

#[async_trait]
impl<T> UseCase<HentJournalpostRequest, JournalpostResponse> for T
where
    T: application::ports::use_cases::HentJournalpostUseCase + Send + Sync,
{
    async fn handle(
        &self,
        req: HentJournalpostRequest,
    ) -> Result<JournalpostResponse, anyhow::Error> {
        application::ports::use_cases::HentJournalpostUseCase::handle(self, req).await
    }
}

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
    U: UseCase<Req, Res> + Send + Sync,
    Req: serde::de::DeserializeOwned + Send,
    Res: serde::Serialize + Send,
{
    pub async fn run(&self) -> anyhow::Result<()> {
        let mut sub = self.client.inner().subscribe(self.subject.clone()).await?;

        while let Some(msg) = sub.next().await {
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
                    // For now: ignore bad requests, no reply
                    continue;
                }
            };

            let result = self.use_case.handle(req).await;

            let nats_response: NatsResponse = match result {
                Ok(payload) => match serde_json::to_vec(&payload) {
                    Ok(bytes) => NatsResponse::Ok(bytes),
                    Err(e) => {
                        error!("Failed to serialize response payload: {e}");
                        NatsResponse::Error(format!("serialize error: {e}").into_bytes())
                    }
                },
                Err(e) => {
                    error!("Use case returned error: {e:?}");
                    NatsResponse::Error(e.to_string().into_bytes())
                }
            };

            let bytes: Vec<u8> = match &nats_response {
                NatsResponse::Ok(b) => b.clone(),
                NatsResponse::Error(b) => b.clone(),
            };

            if let Err(e) = self
                .client
                .inner()
                .publish(reply_subject, bytes.into())
                .await
            {
                error!("Failed to publish reply: {:?}", e);
            } else {
                tracing::info!("Successfully replied with: {:?}", nats_response);
            }
        }

        Ok(())
    }
}
