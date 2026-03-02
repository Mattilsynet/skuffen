#[derive(Clone, Debug)]
pub struct NatsConfig {
    pub server_url: String,
    pub credentials: Option<String>,
    pub require_tls: bool,
}

impl NatsConfig {
    pub fn from_env() -> Result<Self, std::env::VarError> {
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
        Ok(Self {
            server_url: std::env::var("NATS_URL")?,
            credentials,
            require_tls,
        })
    }

    pub fn new(server_url: &str, credentials: Option<&str>) -> Self {
        Self {
            server_url: server_url.to_string(),
            credentials: credentials.map(|c| c.to_string()),
            require_tls: true,
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
