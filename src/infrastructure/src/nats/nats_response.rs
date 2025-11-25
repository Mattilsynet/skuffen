use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum NatsResponse {
    Ok(Vec<u8>),
    Error(Vec<u8>),
}
