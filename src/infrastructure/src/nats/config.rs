use anyhow::{Context, ensure};

const DEFAULT_JETSTREAM_REPLICAS: usize = 3;

#[derive(Clone, Debug)]
pub struct NatsConfig {
    pub server_url: String,
    pub credentials: Option<String>,
    pub require_tls: bool,
    pub jetstream_replicas: usize,
}

impl NatsConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let require_tls = !is_local_env();
        let credentials = std::env::var("APP_NATS_CREDENTIALS")
            .ok()
            .and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });

        let jetstream_replicas = match std::env::var("APP_NATS_JETSTREAM_REPLICAS") {
            Ok(value) => {
                let trimmed = value.trim();
                let replicas = trimmed.parse::<usize>().with_context(|| {
                    format!(
                        "APP_NATS_JETSTREAM_REPLICAS must be a positive integer, got '{trimmed}'"
                    )
                })?;
                ensure!(
                    replicas > 0,
                    "APP_NATS_JETSTREAM_REPLICAS must be greater than zero"
                );
                replicas
            }
            Err(std::env::VarError::NotPresent) => DEFAULT_JETSTREAM_REPLICAS,
            Err(err) => {
                return Err(
                    anyhow::Error::new(err).context("APP_NATS_JETSTREAM_REPLICAS is invalid")
                );
            }
        };

        Ok(Self {
            server_url: std::env::var("NATS_URL").context("missing NATS_URL")?,
            credentials,
            require_tls,
            jetstream_replicas,
        })
    }

    pub fn new(server_url: &str, credentials: Option<&str>) -> Self {
        Self {
            server_url: server_url.to_string(),
            credentials: credentials.map(|c| c.to_string()),
            require_tls: true,
            jetstream_replicas: DEFAULT_JETSTREAM_REPLICAS,
        }
    }
}

fn is_local_env() -> bool {
    let env = match std::env::var("APP_ENV") {
        Ok(value) => value,
        Err(_) => return false,
    };
    env.trim().eq_ignore_ascii_case("local")
}
