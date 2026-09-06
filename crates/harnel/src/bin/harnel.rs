use harnel::{Error, Harness, Provider, Result, acp};
use std::{net::SocketAddr, path::PathBuf};

const HELP: &str = "Harnel: an embeddable agent harness\n\nUsage:\n  harnel acp [OPTIONS]\n  harnel run <PROMPT> [OPTIONS]\n  harnel acp --connect <ADDRESS>\n\nOptions:\n  --workspace <PATH>      Working directory (default: current directory)\n  --state-dir <PATH>      Persistent profile directory (default: temporary)\n  --model <MODEL>         Provider model ID\n  --provider <PROVIDER>   gateway, codex, or grok\n  --base-url <URL>        Responses-compatible provider base URL\n  --api-key-env <NAME>    Read API key from the named environment variable\n  --no-native-tools      Disable built-in tools\n  --stdio                Serve ACP on stdin/stdout (default without --listen)\n  --listen <ADDRESS>     Also serve ACP over loopback TCP\n  --connect <ADDRESS>    Bridge stdio to an existing listener\n  --help                 Print this help\n  --version              Print version and native revision\n";

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("harnel: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "--help".into());
    if command == "--help" || command == "-h" {
        print!("{HELP}");
        return Ok(());
    }
    if command == "--version" {
        println!(
            "harnel {} (fx {})",
            env!("CARGO_PKG_VERSION"),
            harnel_sys::revision()
        );
        return Ok(());
    }
    if command != "acp" && command != "run" {
        return Err(Error::Invalid(format!(
            "unknown command {command}; use --help"
        )));
    }
    let prompt = if command == "run" {
        Some(
            args.next()
                .ok_or_else(|| Error::Invalid("run requires a prompt".into()))?,
        )
    } else {
        None
    };
    let mut workspace = std::env::current_dir()?;
    let mut state = None;
    let mut model = None;
    let mut provider = Provider::Gateway;
    let mut url = None;
    let mut api_key = None;
    let mut native_tools = true;
    let mut stdio = false;
    let mut listen: Option<SocketAddr> = None;
    let mut connect: Option<SocketAddr> = None;
    while let Some(flag) = args.next() {
        if flag == "--help" {
            print!("{HELP}");
            return Ok(());
        }
        if flag == "--stdio" {
            stdio = true;
            continue;
        }
        if flag == "--no-native-tools" {
            native_tools = false;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| Error::Invalid(format!("{flag} requires a value")))?;
        match flag.as_str() {
            "--workspace" => workspace = PathBuf::from(value),
            "--state-dir" => state = Some(PathBuf::from(value)),
            "--model" => model = Some(value),
            "--provider" => {
                provider = match value.as_str() {
                    "gateway" => Provider::Gateway,
                    "codex" => Provider::Codex,
                    "grok" => Provider::Grok,
                    _ => {
                        return Err(Error::Invalid(
                            "provider must be gateway, codex, or grok".into(),
                        ));
                    }
                }
            }
            "--base-url" => url = Some(value),
            "--api-key-env" => {
                api_key = Some(std::env::var(&value).map_err(|_| {
                    Error::Invalid(format!(
                        "environment variable {value} is unset or not UTF-8"
                    ))
                })?)
            }
            "--listen" => {
                listen = Some(
                    value
                        .parse()
                        .map_err(|_| Error::Invalid("invalid listener address".into()))?,
                )
            }
            "--connect" => {
                connect = Some(
                    value
                        .parse()
                        .map_err(|_| Error::Invalid("invalid connection address".into()))?,
                )
            }
            _ => return Err(Error::Invalid(format!("unknown option {flag}"))),
        }
    }
    if let Some(address) = connect {
        if command != "acp" || listen.is_some() {
            return Err(Error::Invalid(
                "--connect requires acp without --listen".into(),
            ));
        }
        return acp::bridge_stdio(address).await;
    }
    if command == "run" && (stdio || listen.is_some()) {
        return Err(Error::Invalid("transport options require acp".into()));
    }
    let mut builder = Harness::builder(workspace)
        .provider(provider)
        .native_tools(native_tools);
    if let Some(path) = state {
        builder = builder.state_dir(path);
    }
    if let Some(model) = model {
        builder = builder.model(model);
    }
    if let Some(url) = url {
        builder = builder.base_url(url);
    }
    if let Some(key) = api_key {
        builder = builder.api_key(key);
    }
    let harness = builder.build().await?;
    let result = if let Some(prompt) = prompt {
        prompt_once(&harness, prompt).await
    } else {
        let listener = if let Some(address) = listen {
            let listener = harness.listen(address).await?;
            eprintln!("ACP listening on {}", listener.local_addr());
            Some(listener)
        } else {
            None
        };
        let result = if stdio || listener.is_none() {
            tokio::select! {
                result = harness.serve_stdio() => {
                    if listener.is_some() {
                        if let Err(error) = result { eprintln!("stdio attachment closed: {error}"); }
                        tokio::signal::ctrl_c().await.map_err(Error::from)
                    } else { result }
                },
                result = tokio::signal::ctrl_c() => result.map_err(Error::from),
            }
        } else {
            tokio::signal::ctrl_c().await.map_err(Error::from)
        };
        if let Some(listener) = listener {
            listener.shutdown().await?;
        }
        result
    };
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}

async fn prompt_once(harness: &Harness, prompt: String) -> Result<()> {
    let session = harness.session().await?;
    let mut events = harness.subscribe();
    let turn = session.prompt(prompt);
    tokio::pin!(turn);
    loop {
        tokio::select! {
            result = &mut turn => {
                result?;
                while let Some(event) = events.try_recv()? { print_event(&event)?; }
                break;
            },
            result = tokio::signal::ctrl_c() => { result?; session.cancel()?; return Err(Error::Invalid("interrupted".into())); },
            event = events.recv() => {
                let event = event?;
                print_event(&event)?;
            }
        }
    }
    println!();
    Ok(())
}

fn print_event(event: &harnel::Event) -> Result<()> {
    if event.params["update"]["sessionUpdate"] == "agent_message_chunk" {
        if let Some(text) = event.params["update"]["content"]["text"].as_str() {
            use std::io::Write;
            print!("{text}");
            std::io::stdout().flush()?;
        }
    }
    Ok(())
}
