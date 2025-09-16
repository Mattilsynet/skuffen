use axum::{http::StatusCode, routing::get, Router};
use std::net::SocketAddr;
use tokio::{net::TcpListener, task::JoinHandle};

pub async fn health_check(addr: SocketAddr) -> std::io::Result<JoinHandle<()>> {
    let listener = TcpListener::bind(addr).await?;

    let app = Router::new().route("/", get(|| async { StatusCode::OK }));

    let handle = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("health server error: {err}");
        }
    });

    Ok(handle)
}
