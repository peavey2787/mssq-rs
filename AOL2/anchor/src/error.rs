use thiserror::Error;

pub type Result<T> = std::result::Result<T, AnchorError>;

#[derive(Debug, Error)]
pub enum AnchorError {
    #[error("invalid Kaspa network `{0}`")]
    InvalidNetwork(String),
    #[error("Kaspa RPC error: {0}")]
    KaspaRpc(String),
    #[error("Kaspa wallet error: {0}")]
    KaspaWallet(String),
    #[error("Kaspa indexer error: {0}")]
    Indexer(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("JSON error: {0}")]
    Json(String),
}