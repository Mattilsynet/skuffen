use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "status", content = "payload")]
pub enum NatsResponse<T> {
    Ok(T),
    Error { message: String },
}
