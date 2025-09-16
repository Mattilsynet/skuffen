use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    let http_handle = health_check::health_check(addr).await?;
    let nats_handle = read_worker::infrastucture::nats::setup::setup_nats_replier();

    let _ = tokio::join!(http_handle, nats_handle);
    Ok(())
}
