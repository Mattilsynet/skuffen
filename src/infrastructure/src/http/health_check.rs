use axum::{Router, extract::State, http::StatusCode, routing::get};
use std::{env, net::SocketAddr};
use tokio::{net::TcpListener, task::JoinHandle};

use crate::http::helse::Helse;

/// Porten bindes først av alt (SKU-0021 R6), så Cloud Runs startup-probe kan
/// nå tjenesten mens migrasjonene fortsatt kjører.
pub async fn health_check(helse: Helse) -> std::io::Result<JoinHandle<()>> {
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

    let handle = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, helse_router(helse)).await {
            eprintln!("health server error: {err}");
        }
    });

    Ok(handle)
}

/// `/` beholdes som alias for liveness til Cloud Run-probene er utrullet.
fn helse_router(helse: Helse) -> Router {
    Router::new()
        .route("/", get(liveness))
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .with_state(helse)
}

/// Beviser bare at runtimen svarer. Avhenger av ingenting — en liveness som
/// feiler på en nede database ville gitt en restartloop som ikke reparerer noe.
async fn liveness() -> StatusCode {
    StatusCode::OK
}

async fn readiness(State(helse): State<Helse>) -> StatusCode {
    if helse.er_klar() {
        return StatusCode::OK;
    }

    tracing::debug!(nede = ?helse.nede(), "readiness er usann");
    StatusCode::SERVICE_UNAVAILABLE
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn status(helse: Helse, sti: &str) -> StatusCode {
        helse_router(helse)
            .oneshot(Request::builder().uri(sti).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn liveness_svarer_selv_naar_ingenting_er_oppe() {
        let helse = Helse::new();
        helse.registrer_task("command_listener");

        assert_eq!(status(helse.clone(), "/health/live").await, StatusCode::OK);
        assert_eq!(status(helse, "/").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn readiness_er_usann_til_migrasjonene_er_ferdige() {
        let helse = Helse::new();
        helse.sett_nats(true);

        assert_eq!(
            status(helse.clone(), "/health/ready").await,
            StatusCode::SERVICE_UNAVAILABLE
        );

        helse.sett_migrert(true);
        assert_eq!(status(helse, "/health/ready").await, StatusCode::OK);
    }
}
