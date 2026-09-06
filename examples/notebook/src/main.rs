mod notebook;

use harnel::{Error, Event, Harness, Result, Session};
use notebook::Notebook;
use std::{
    io::{self, Write},
    path::PathBuf,
};

fn required(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Invalid(format!("Set {name}; see README.md")))
}

fn display(session: &Session, event: Event) -> Result<()> {
    if event.params["sessionId"] == session.id()
        && event.params["update"]["sessionUpdate"] == "agent_message_chunk"
    {
        if let Some(text) = event.params["update"]["content"]["text"].as_str() {
            print!("{text}");
            io::stdout().flush()?;
        }
    }
    Ok(())
}

async fn ask(session: &Session, question: String) -> Result<()> {
    let mut events = session.harness().subscribe();
    let turn = session.prompt(question);
    tokio::pin!(turn);
    loop {
        tokio::select! {
            result = &mut turn => {
                let turn = result?;
                while let Some(event) = events.try_recv()? {
                    display(session, event)?;
                }
                println!();
                eprintln!("Stop reason: {}", turn.stop_reason);
                return Ok(());
            }
            event = events.recv() => display(session, event?)?,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err(Error::Invalid(
            "Usage: harnel-notebook <notes-directory> <question> [session-id]".into(),
        ));
    }
    let directory = PathBuf::from(&args[0]).canonicalize()?;
    let notebook = Notebook::load(&directory)?;
    let profile = std::env::var_os("HARNEL_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join(".harnel-state"));
    let harness = Harness::builder(&directory)
        .state_dir(profile)
        .model(required("HARNEL_MODEL")?)
        .base_url(required("HARNEL_BASE_URL")?)
        .api_key(required("HARNEL_API_KEY")?)
        .instructions(notebook.instructions())
        .native_tools(false)
        .tool(notebook)
        .build()
        .await?;
    let result = async {
        let session = match args.get(2) {
            Some(id) => harness.load_session(id).await?,
            None => harness.session().await?,
        };
        eprintln!("Session: {}", session.id());
        eprintln!("Native revision: {}", harness.native_revision());
        let listener = match std::env::var("HARNEL_LISTEN") {
            Ok(address) => {
                let address = address
                    .parse()
                    .map_err(|_| Error::Invalid("Invalid HARNEL_LISTEN address".into()))?;
                let listener = harness.listen(address).await?;
                eprintln!("ACP listening on {}", listener.local_addr());
                Some(listener)
            }
            Err(_) => None,
        };
        let result = ask(&session, args[1].clone()).await;
        let detach = match listener {
            Some(listener) => listener.shutdown().await,
            None => Ok(()),
        };
        result.and(detach)
    }
    .await;
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}
