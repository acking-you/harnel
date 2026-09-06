//! Persist a BYOK endpoint/key binding and observe it from a shared SDK handle.
mod common;

use harnel::{Harness, Provider, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let harness = Harness::builder(std::env::current_dir()?)
        .state_dir(common::required("HARNEL_STATE_DIR")?)
        .model(common::required("HARNEL_MODEL")?)
        .native_tools(false)
        .build()
        .await?;
    let result = async {
        harness
            .configure_provider(
                &common::required("HARNEL_BASE_URL")?,
                &common::required("HARNEL_API_KEY")?,
            )
            .await?;
        let control = harness.clone();
        control.switch_provider(Provider::Gateway).await?;
        eprintln!(
            "Active provider: {}",
            control.provider_status().await?["provider"]
        );
        let answer = harness
            .session()
            .await?
            .ask(common::prompt("Say hello."))
            .await?;
        println!("{}", answer.text);
        // Switch to Codex/Grok after OAuth, then back to Gateway without
        // losing the persisted BYOK binding.
        Ok(())
    }
    .await;
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}
