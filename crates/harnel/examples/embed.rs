//! Collect an answer with the smallest useful SDK integration.
mod common;

use harnel::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let harness = common::builder()?.build().await?;
    let result = async {
        let session = harness.session().await?;
        let answer = session
            .ask(common::prompt("Say hello in one sentence."))
            .await?;
        println!("{}", answer.text);
        eprintln!("Stop reason: {}", answer.turn.stop_reason);
        Ok(())
    }
    .await;
    // Settle native workers even if the provider rejected the request.
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}
