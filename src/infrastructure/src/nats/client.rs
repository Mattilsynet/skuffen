use std::time::Duration;

use async_nats::{Client, ConnectOptions};

use super::config::{NatsConfig, safe_nats_server_label};

#[derive(Clone, Debug)]
pub struct NatsClient {
    client: Client,
    jetstream_replicas: usize,
}

impl NatsClient {
    pub async fn connect(config: &NatsConfig) -> Result<Self, async_nats::Error> {
        let name = "Skuffen-read-worker";
        tracing::info!("Connecting to nats cluster as {name}");
        let mut options = ConnectOptions::new()
            .name(name)
            .connection_timeout(Duration::from_secs(5))
            .require_tls(config.require_tls);

        if let Some(ref creds) = config.credentials {
            options = options.credentials(creds.as_str()).unwrap();
        }

        let client = options.connect(&config.server_url).await?;

        tracing::info!(
            nats_server = %safe_nats_server_label(&config.server_url),
            "Successfully connected read worker to NATS server"
        );

        Ok(Self {
            client,
            jetstream_replicas: config.jetstream_replicas,
        })
    }

    pub fn inner(&self) -> &Client {
        &self.client
    }

    pub fn jetstream_replicas(&self) -> usize {
        self.jetstream_replicas
    }
}
