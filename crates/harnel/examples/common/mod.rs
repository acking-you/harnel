//! Configuration shared by the runnable examples, not part of the SDK.
#![allow(dead_code)]

use harnel::{Builder, Error, Harness, Result};

pub fn required(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Invalid(format!("Set {name}; see examples/README.md")))
}

pub fn builder() -> Result<Builder> {
    let mut builder = Harness::builder(std::env::current_dir()?)
        .model(required("HARNEL_MODEL")?)
        .base_url(required("HARNEL_BASE_URL")?)
        .api_key(required("HARNEL_API_KEY")?)
        .native_tools(false);
    if let Ok(path) = std::env::var("HARNEL_STATE_DIR") {
        builder = builder.state_dir(path);
    }
    Ok(builder)
}

pub fn prompt(default: &str) -> String {
    std::env::args().nth(1).unwrap_or_else(|| default.into())
}

pub async fn interrupt() -> Result<()> {
    #[cfg(windows)]
    {
        let mut control_c = tokio::signal::windows::ctrl_c()?;
        let mut control_break = tokio::signal::windows::ctrl_break()?;
        tokio::select! {
            _ = control_c.recv() => {},
            _ = control_break.recv() => {},
        }
    }
    #[cfg(not(windows))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}
