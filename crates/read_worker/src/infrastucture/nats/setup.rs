use tokio::task::JoinHandle;
use tracing::info;

use crate::infrastucture::nats::{client::NatsClient, config::NatsConfig, listener};

pub async fn setup_nats_replier() -> Result<JoinHandle<()>, anyhow::Error> {
    info!("Configuring nats replier");
    let config = NatsConfig::from_env()?;
    let client = NatsClient::connect(&config)
        .await
        .expect("failed to connect nats client");

    let handle = tokio::spawn(async move {
        if let Err(e) = listener::serve(client).await {
            tracing::error!("Error with the nats replier server: {e}");
        }
    });

    Ok(handle)
}
