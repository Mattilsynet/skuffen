use std::time::Duration;

use async_nats::{Client, ConnectOptions, Event};

use super::config::{NatsConfig, safe_nats_server_label};
use crate::http::helse::Helse;

#[derive(Clone, Debug)]
pub struct NatsClient {
    client: Client,
    jetstream_replicas: usize,
}

impl NatsClient {
    pub async fn connect(config: &NatsConfig, helse: Helse) -> Result<Self, async_nats::Error> {
        let name = "Skuffen-read-worker";
        tracing::info!("Connecting to nats cluster as {name}");
        let mut options = ConnectOptions::new()
            .name(name)
            .connection_timeout(Duration::from_secs(5))
            .require_tls(config.require_tls)
            // Readiness skal si sant om forbindelsen, ikke om oppstart lyktes
            // én gang (SKU-0021 R5).
            .event_callback({
                let helse = helse.clone();
                move |event| {
                    let helse = helse.clone();
                    async move {
                        match event {
                            Event::Connected => helse.sett_nats(true),
                            Event::Disconnected | Event::Closed => helse.sett_nats(false),
                            _ => {}
                        }
                    }
                }
            });

        if let Some(ref creds) = config.credentials {
            options = options.credentials(creds.as_str()).unwrap();
        }

        let client = options.connect(&config.server_url).await?;
        helse.sett_nats(true);

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
