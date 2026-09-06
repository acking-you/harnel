//! Stream assistant text, drain the final events, and cancel with Ctrl-C.
mod common;

use harnel::{Event, Result};
use std::io::{self, Write};

fn display(session_id: &str, event: Event) -> Result<()> {
    if event.params["sessionId"] == session_id {
        let update = &event.params["update"];
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => {
                if let Some(text) = update["content"]["text"].as_str() {
                    print!("{text}");
                    io::stdout().flush()?;
                }
            }
            Some("tool_call" | "tool_call_update") => {
                eprintln!("Tool: {}", update["title"]);
            }
            _ => {}
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let harness = common::builder()?.build().await?;
    let result = async {
        let session = harness.session().await?;
        // Subscribe first so that even the first chunk is observable.
        let mut events = harness.subscribe();
        let turn = session.prompt(common::prompt("Explain embedded agent harnesses."));
        tokio::pin!(turn);
        let interrupted = common::interrupt()?;
        tokio::pin!(interrupted);
        let mut cancelled = false;
        loop {
            tokio::select! {
                result = &mut turn => {
                    let turn = result?;
                    // A ready reply can race with already queued text events.
                    while let Some(event) = events.try_recv()? {
                        display(session.id(), event)?;
                    }
                    println!();
                    eprintln!("Stop reason: {}", turn.stop_reason);
                    break;
                }
                event = events.recv() => display(session.id(), event?)?,
                result = &mut interrupted, if !cancelled => {
                    result?;
                    session.cancel()?;
                    eprintln!("Cancelling turn...");
                    cancelled = true;
                    // Keep receiving until the native turn acknowledges cancellation.
                }
            }
        }
        Ok(())
    }
    .await;
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}
