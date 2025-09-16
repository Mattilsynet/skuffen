use std::time::Duration;

use async_nats::{Client, ConnectOptions};

use super::config::NatsConfig;

#[derive(Clone)]
pub struct NatsClient {
    client: Client,
}

impl NatsClient {
    pub async fn connect(config: &NatsConfig) -> Result<Self, async_nats::Error> {
        let name = "Skuffen-read-worker";
        tracing::info!("Connecting to nats cluster as {name}");
        let mut options = ConnectOptions::new()
            .name(name)
            .connection_timeout(Duration::from_secs(5))
            .require_tls(true);

        if let Some(ref creds) = config.credentials {
            options = options.credentials(creds.as_str()).unwrap();
        }

        let client = options.connect(&config.server_url).await?;

        tracing::info!(
            "Successfully connected read worker to NATS server: {}",
            config.server_url
        );

        Ok(Self { client })
    }

    pub fn inner(&self) -> &Client {
        &self.client
    }
}
