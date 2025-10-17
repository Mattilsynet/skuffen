use futures::StreamExt;

use crate::infrastucture::nats::client::NatsClient;

pub async fn serve(client: NatsClient) -> Result<(), anyhow::Error> {
    let request_subject = "arkiv.read";
    let mut requests = client.inner().subscribe(request_subject).await.unwrap();
    tracing::info!("Replying to {}", request_subject);
    while let Some(request) = requests.next().await {
        if let Some(reply) = request.clone().reply {
            let response = "hello";
            if let Err(e) = client.inner().publish(reply, response.into()).await {
                tracing::error!("Failed to publish reply: {:?}", e);
            } else {
                tracing::info!("Successfully replied with: {}", response);
            }
        } else {
            tracing::error!("Request has no reply subejct attached.");
        }
    }
    Ok(())
}
