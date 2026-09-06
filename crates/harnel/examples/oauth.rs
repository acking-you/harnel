//! Authorize Codex or Grok from another device, then reuse the saved profile.
mod common;

use harnel::{Error, Harness, Provider, Result};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let provider = match std::env::args().nth(1).as_deref() {
        Some("codex") => Provider::Codex,
        Some("grok") => Provider::Grok,
        _ => return Err(Error::Invalid("Usage: oauth <codex|grok>".into())),
    };
    let harness = Harness::builder(std::env::current_dir()?)
        .state_dir(common::required("HARNEL_STATE_DIR")?)
        .native_tools(false)
        .build()
        .await?;
    let result = async {
        // Device code is the default: no local browser or callback server.
        let initial = harness.login(provider).await?;
        if initial["state"] == "polling" {
            eprintln!(
                "Open {} and enter {}",
                initial["verificationUri"], initial["userCode"]
            );
        }
        let expires = initial["expiresIn"].as_u64().unwrap_or(600).min(3600);
        let deadline = tokio::time::sleep(Duration::from_secs(expires));
        tokio::pin!(deadline);
        let interrupted = common::interrupt()?;
        tokio::pin!(interrupted);
        loop {
            let status = harness.login_status().await?;
            match status["state"].as_str() {
                Some("succeeded") => {
                    harness.switch_provider(provider).await?;
                    println!("Login succeeded. Credentials are saved in the selected profile.");
                    break;
                }
                Some("preparing" | "polling") => {}
                _ => {
                    return Err(Error::Invalid(format!(
                        "Login ended with state {}",
                        status["state"]
                    )));
                }
            }
            tokio::select! {
                result = &mut interrupted => {
                    result?;
                    harness.cancel_login().await?;
                    return Err(Error::Invalid("Login cancelled".into()));
                }
                _ = &mut deadline => {
                    harness.cancel_login().await?;
                    return Err(Error::Timeout);
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
        Ok(())
    }
    .await;
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}
