use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{message} (RPC {code})")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Native(#[from] harnel_sys::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Rpc(#[from] RpcError),
    #[error("harness is closed")]
    Closed,
    #[error("{0}")]
    Invalid(String),
    #[error("request capacity exceeded")]
    Busy,
    #[error("event subscriber missed {0} events")]
    Lagged(u64),
    #[error("request timed out")]
    Timeout,
}

impl Error {
    pub(crate) fn rpc(self) -> RpcError {
        match self {
            Self::Rpc(error) => error,
            Self::Invalid(message) => RpcError {
                code: -32602,
                message,
                data: None,
            },
            error => RpcError {
                code: -32603,
                message: error.to_string(),
                data: None,
            },
        }
    }
}
