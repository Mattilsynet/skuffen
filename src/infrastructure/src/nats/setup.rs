use tracing::info;

use crate::http::helse::Helse;
use crate::nats::{client::NatsClient, config::NatsConfig};

pub async fn setup_nats(helse: Helse) -> Result<NatsClient, anyhow::Error> {
    info!("Configuring nats");
    let config = NatsConfig::from_env()?;
    let client = NatsClient::connect(&config, helse)
        .await
        .map_err(|err| anyhow::anyhow!("failed to connect nats client: {err}"))?;

    Ok(client)
}
