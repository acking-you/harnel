//! Harnel embeds the native fx agent runtime and exposes one shared control
//! surface through Rust, ACP stdio, and local socket connections.
//!
//! Start with [`Harness::builder`], create a [`Session`], and submit a prompt.
//! Subscribe before prompting to receive streamed text, reasoning, tool, and
//! lifecycle events. [`Harness::request`] exposes the full native ACP surface.
#![forbid(unsafe_code)]

pub mod acp;
mod error;
mod runtime;
mod session;
pub mod tool;

pub use error::{Error, Result, RpcError};
pub use runtime::{Builder, Event, Events, Harness, Provider};
pub use serde_json::{Value, json};
pub use session::{Answer, Session, TurnResult};
