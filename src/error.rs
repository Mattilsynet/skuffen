use serde::{Deserialize, Serialize};
use thiserror::Error;

use lib_schemas::error::SchemasError;

pub type Result<T> = core::result::Result<T, SkuffenError>;

#[derive(Error, Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum SkuffenError {
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Parse error")]
    Parse(#[from] ParseError),

    #[error("Schemas error")]
    Schemas(#[from] SchemasError),
}

#[derive(Error, Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum ParseError {
    #[error("{0}")]
    Message(String),
}

impl From<String> for ParseError {
    fn from(s: String) -> Self {
        ParseError::Message(s)
    }
}
