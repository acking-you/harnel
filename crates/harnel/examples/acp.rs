//! Attach stdio and a loopback listener to one SDK-owned runtime.
mod common;

use harnel::{Error, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let address = std::env::var("HARNEL_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:7788".into())
        .parse()
        .map_err(|_| Error::Invalid("HARNEL_LISTEN must be an IP address and port".into()))?;
    let harness = common::builder()?.build().await?;
    let result = async {
        let session = harness.session().await?;
        let listener = harness.listen(address).await?;
        // stdout is reserved for ACP. Log SDK observations to stderr.
        eprintln!("ACP listening on {}", listener.local_addr());
        eprintln!("SDK session: {}", session.id());
        let stdio = harness.serve_stdio();
        tokio::pin!(stdio);
        let interrupted = common::interrupt();
        tokio::pin!(interrupted);
        let mut stdio_open = true;
        let result = loop {
            tokio::select! {
                result = &mut interrupted => break result,
                result = &mut stdio, if stdio_open => {
                    if let Err(error) = result { break Err(error); }
                    stdio_open = false;
                    // EOF detaches stdio while listener clients keep working.
                }
            }
        };
        let detach = listener.shutdown().await;
        result.and(detach)
    }
    .await;
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}
