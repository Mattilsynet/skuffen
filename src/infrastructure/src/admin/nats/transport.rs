use std::pin::Pin;

use async_nats::HeaderMap;
use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};

use crate::nats::client::NatsClient;

/// Én innkommende admin-request, uavhengig av NATS-klienten.
pub struct AdminMessage {
    pub reply: Option<String>,
    pub headers: Option<HeaderMap>,
    pub payload: Bytes,
}

pub type AdminMessageStream = Pin<Box<dyn Stream<Item = AdminMessage> + Send>>;

/// Liten grense rundt subscribe/publish, slik at handler- og
/// subscription-oppførsel kan testes uten en NATS-server.
#[async_trait]
pub trait AdminTransport: Send + Sync {
    async fn queue_subscribe(
        &self,
        subject: &'static str,
        queue_group: &'static str,
    ) -> anyhow::Result<AdminMessageStream>;

    async fn publish(&self, reply: String, payload: Vec<u8>) -> anyhow::Result<()>;

    fn max_payload(&self) -> usize;
}

pub struct NatsAdminTransport {
    client: NatsClient,
}

impl NatsAdminTransport {
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl AdminTransport for NatsAdminTransport {
    async fn queue_subscribe(
        &self,
        subject: &'static str,
        queue_group: &'static str,
    ) -> anyhow::Result<AdminMessageStream> {
        let subscription = self
            .client
            .inner()
            .queue_subscribe(subject, queue_group.to_string())
            .await?;
        Ok(Box::pin(subscription.map(|message| AdminMessage {
            reply: message.reply.map(|reply| reply.to_string()),
            headers: message.headers,
            payload: message.payload,
        })))
    }

    async fn publish(&self, reply: String, payload: Vec<u8>) -> anyhow::Result<()> {
        self.client
            .inner()
            .publish(reply, payload.into())
            .await
            .map_err(anyhow::Error::from)
    }

    fn max_payload(&self) -> usize {
        self.client.inner().max_payload()
    }
}
