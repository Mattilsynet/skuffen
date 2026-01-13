use axum::{Router, http::StatusCode, routing::get};
use std::{env, net::SocketAddr};
use tokio::{net::TcpListener, task::JoinHandle};

pub async fn health_check() -> std::io::Result<JoinHandle<()>> {
    let host = env::var("APP_APPLICATION__HOST").unwrap_or_else(|_| {
        panic!("Miljøvariabelen APP_APPLICATION__HOST er ikke satt. Sett denne før oppstart");
    });

    let port: u16 = env::var("APP_APPLICATION__PORT")
        .unwrap_or_else(|_| {
            panic!("Miljøvariabelen APP_APPLICATION__PORT er ikke satt. Sett denne før oppstart");
        })
        .parse()
        .unwrap_or_else(|_| panic!("APP_APPLICATION__PORT må være et tall"));

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .unwrap_or_else(|_| panic!("Kunne ikke parse {host}:{port} som SocketAddr"));

    let listener = TcpListener::bind(addr).await?;

    let app = Router::new().route("/", get(|| async { StatusCode::OK }));

    let handle = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("health server error: {err}");
        }
    });

    Ok(handle)
}
