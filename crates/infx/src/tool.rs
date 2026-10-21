use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{future::Future, pin::Pin};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    /// Only mark tools without side effects as read-only.
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub session_id: String,
    pub tool_call_id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolOutput {
    pub output: String,
    pub is_error: bool,
}
impl ToolOutput {
    pub fn text(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
        }
    }
    pub fn failure(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
        }
    }
}

/// Host tools use the same fx admission and permission policy as native tools.
/// Cancellation drops the returned future; implementations should not detach
/// side effects that cannot be cancelled or observed by the host application.
pub trait Tool: Send + Sync + 'static {
    fn spec(&self) -> ToolSpec;
    fn execute(&self, call: ToolCall) -> BoxFuture<'_, Result<ToolOutput>>;
}

/// Handles agent-to-host requests, including `session/request_permission`.
/// The default handler rejects permission and fails unsupported requests.
pub trait ClientHandler: Send + Sync + 'static {
    fn request(&self, method: String, params: Value) -> BoxFuture<'_, Result<Value>>;
}

pub(crate) struct DefaultHandler;
impl ClientHandler for DefaultHandler {
    fn request(&self, method: String, _: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            if method == "session/request_permission" {
                Ok(serde_json::json!({"outcome":{"outcome":"cancelled"}}))
            } else {
                Err(crate::Error::Invalid(format!(
                    "No host handler for {method}"
                )))
            }
        })
    }
}
