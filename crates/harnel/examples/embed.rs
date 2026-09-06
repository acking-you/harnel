use harnel::{Harness, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let harness = Harness::builder(std::env::current_dir()?)
        .model(std::env::var("HARNEL_MODEL").unwrap_or_else(|_| "openai/gpt-5".into()))
        .base_url(std::env::var("HARNEL_BASE_URL").expect("set HARNEL_BASE_URL"))
        .api_key(std::env::var("HARNEL_API_KEY").expect("set HARNEL_API_KEY"))
        .build()
        .await?;
    let session = harness.session().await?;
    let listener = harness.listen("127.0.0.1:0".parse().unwrap()).await?;
    eprintln!("ACP attachment: {}", listener.local_addr());
    let mut events = harness.subscribe();
    let turn = session.prompt("Describe the current workspace.");
    tokio::pin!(turn);
    loop {
        tokio::select! {
            result = &mut turn => { println!("Stop reason: {}", result?.stop_reason); break; },
            event = events.recv() => { let event = event?; println!("{}: {}", event.method, event.params); },
        }
    }
    listener.shutdown().await?;
    harness.shutdown().await
}
