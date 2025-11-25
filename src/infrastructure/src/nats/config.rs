#[derive(Clone, Debug)]
pub struct NatsConfig {
    pub server_url: String,
    pub credentials: Option<String>,
}

impl NatsConfig {
    pub fn from_env() -> Result<Self, std::env::VarError> {
        Ok(Self {
            server_url: std::env::var("NATS_URL")?,
            credentials: std::env::var("APP_NATS_CREDENTIALS").ok(),
        })
    }

    pub fn new(server_url: &str, credentials: Option<&str>) -> Self {
        Self {
            server_url: server_url.to_string(),
            credentials: credentials.map(|c| c.to_string()),
        }
    }
}
