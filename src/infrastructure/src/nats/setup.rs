use tracing::info;

use crate::nats::{client::NatsClient, config::NatsConfig};

pub async fn setup_nats() -> Result<NatsClient, anyhow::Error> {
    info!("Configuring nats");
    let config = NatsConfig::from_env()?;
    let client = NatsClient::connect(&config)
        .await
        .expect("failed to connect nats client");

    Ok(client)
}
