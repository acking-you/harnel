//! Run twice with the same profile to continue a saved conversation.
mod common;

use infx::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let harness = common::builder()?
        .state_dir(common::required("INFX_STATE_DIR")?)
        .build()
        .await?;
    let result = async {
        let session = match std::env::var("INFX_SESSION_ID") {
            Ok(id) => harness.load_session(id).await?,
            Err(_) => harness.session().await?,
        };
        eprintln!("Session: {}", session.id());
        let answer = session
            .ask(common::prompt("Remember that my project is named Atlas."))
            .await?;
        println!("{}", answer.text);
        eprintln!("Status: {}", session.status().await?["state"]);
        // close unloads the session; remove would delete its saved data.
        session.close().await?;
        Ok(())
    }
    .await;
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}
