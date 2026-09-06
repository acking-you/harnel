# Harnel

An embeddable agent harness for Rust, powered by [fx](https://github.com/acking-you/fx).

[![CI](https://github.com/acking-you/harnel/actions/workflows/ci.yml/badge.svg)](https://github.com/acking-you/harnel/actions/workflows/ci.yml)

## Why Harnel exists

Applications should be able to add an agent as easily as they add an embedded
database. Harnel aims to bring the qualities that make SQLite useful to the
agent harness: a small native runtime, a direct library interface, explicit
ownership of state, and no mandatory service to operate.

The application supplies its model credentials, tools, instructions, and user
interface. Harnel supplies the execution loop, session state, streaming events,
context management, and provider controls. Rust code and ACP clients operate on
the same running harness through SDK, stdio, and listener interfaces.

Harnel reuses fx's Zig implementation instead of maintaining a second agent
loop. The fx fork is a Git submodule pinned to an exact revision. Shared engine
changes belong in fx; Rust ergonomics, transport attachment, and distribution
belong here.

## Status and requirements

This is an initial development version. The repository contains a working native
runtime integration, Rust SDK, ACP transports, and deterministic integration
tests. It has **not been published to crates.io**. Public APIs may change before
the first release.

Default builds require Rust 1.85 or newer and the platform linker. They use
precompiled static libraries from **fx v0.0.8**, selected by Cargo's target.
No Zig compiler, fx executable, or build-time download script is needed by a
packaged dependency. Cargo downloads and caches the platform crate along with
ordinary dependencies; after dependencies are prepared, builds can run offline.

Supported release targets are Linux GNU on x86_64/aarch64 (glibc 2.28 or newer),
macOS on x86_64/aarch64 (macOS 13 or newer), and Windows x86_64 MSVC (Windows 10
or newer, dynamic MSVC CRT). Windows requires the normal Rust MSVC linker and
Windows SDK. Libraries use baseline CPUs and stripped ReleaseSafe code.
Running an application requires neither Zig nor a separate agent service.

The native release lock records the fx revision, ABI, and archive/library
checksums. Platform crates contain the original release library, C header,
manifest, and license notices. Build scripts validate the target, ABI, revision,
and checksums before linking. The release library is embedded in the final
application; it is not loaded from a separately installed fx service.

Source builds are an explicit option. They compile the submodule in a checkout,
or the bundled source in a packaged crate. They use Zig **0.16.0**, automatically
obtaining the pinned, checksum-verified compiler when it is not already
available. Downloads and compiler files stay in Cargo's build output directory.
Set `ZIG` to use an existing compiler, or `HARNEL_OFFLINE=1` to prohibit compiler
downloads. Python 3 is used only for repository preparation and verification.

On Windows, use PowerShell or a developer terminal, and use `python` in place
of `python3` in the repository commands below. Git Bash is recommended for
the native shell tools; fx also discovers installed PowerShell and cmd shells.
The CLI accepts both Ctrl-C and Ctrl-Break for graceful shutdown.

The SQLite comparison describes the product direction: explicit ownership, an
embedded deployment model, and a small integration surface. It is not a claim
of SQLite-equivalent binary size, storage guarantees, or maturity. Package
publication and further size optimization remain work for the first crates.io
release.

## Build and try it

```sh
git clone https://github.com/acking-you/harnel.git
cd harnel
python3 scripts/prepare-native.py
cargo build --release
cargo run --bin harnel -- --help
```

For an existing checkout, run `python3 scripts/prepare-native.py` after updating
to a different native release. Default builds do not need the fx submodule.

```sh
export MY_MODEL_KEY='your-key'
cargo run --bin harnel -- run "Explain this workspace" \
  --workspace . \
  --model openai/gpt-5 \
  --base-url https://your-provider.example/v1 \
  --api-key-env MY_MODEL_KEY
```

BYOK currently uses the OpenAI Responses protocol. An endpoint that implements
only Chat Completions is not sufficient. Select a model actually offered by
your provider.

## Embed in Rust

Until publication, use the local crate after preparing the native release above:

```toml
[dependencies]
harnel = { path = "../harnel/crates/harnel" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust,no_run
use harnel::{Harness, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let harness = Harness::builder(std::env::current_dir()?)
        .model("openai/gpt-5")
        .base_url("https://your-provider.example/v1")
        .api_key(std::env::var("MY_MODEL_KEY").expect("set MY_MODEL_KEY"))
        .state_dir("./agent-state")
        .build().await?;

    let session = harness.session().await?;
    let answer = session.ask("Explain this workspace").await?;
    println!("{}", answer.text);

    harness.shutdown().await
}
```

Omit `state_dir` for a temporary profile. A persistent profile stores fx state
under `<state_dir>/.fx/`, including credentials and saved sessions. Multiple
`Harness` instances have independent configuration; clones share their engine.
Harnel does not change the application's environment or working directory.
Provider environment variables must be supplied explicitly through builder
methods or `Builder::env`.

`ask` collects up to 8 MiB of assistant text. For streaming, subscribe before
calling `session.prompt(...)` and consume `Events::recv()` concurrently. Events
include text, reasoning, tool progress, permission-related updates, and session
lifecycle notifications. See [the embedding example](crates/harnel/examples/embed.rs).

## ACP and SDK share live state

```rust,no_run
# async fn example(harness: harnel::Harness) -> harnel::Result<()> {
let session = harness.session().await?;
let listener = harness.listen("127.0.0.1:7788".parse().unwrap()).await?;

// An ACP client can now control the session created by the SDK.
let status = session.status().await?;
println!("{status}");

listener.shutdown().await?; // Detach clients; keep the harness alive.
harness.shutdown().await
# }
```

```sh
# ACP over stdin/stdout
harnel acp --stdio --workspace . --state-dir ./agent-state

# A listener, or stdio and a listener attached to the same engine
harnel acp --listen 127.0.0.1:7788 --workspace . --state-dir ./agent-state
harnel acp --stdio --listen 127.0.0.1:7788 --workspace . --state-dir ./agent-state

# Connect a stdio-only ACP client to the existing listener
harnel acp --connect 127.0.0.1:7788
```

These installed-binary commands correspond to `cargo run --bin harnel -- ...`
when developing from source. Diagnostics go to stderr; ACP stdout contains only
protocol messages.

The listener is Harnel's loopback TCP transport for newline-delimited ACP
JSON-RPC. It is a trusted local control interface and grants access to the
entire harness. Public binds are rejected. Network framing is a Harnel transport
extension, not a claim that every ACP client supports TCP natively.

Each attachment initializes independently and owns its request IDs. Replies
return to their caller; public events are broadcast. Permission requests go to
the client that started the turn, or the SDK's configured `ClientHandler`.
Disconnecting cancels that client's outstanding prompts and preserves the
engine, provider configuration, and saved session.

One harness has **one loaded session and one active root turn**. Creating or
loading a session changes that shared selection for every attachment. Use
separate harnesses with separate state directories for independent concurrent
root sessions. Concurrent native subagents remain part of the fx runtime.

## Providers and authentication

```rust,no_run
# async fn example(harness: harnel::Harness) -> harnel::Result<()> {
use harnel::Provider;

// Validate, persist, and activate a BYOK URL/key pair.
harness.configure_provider("https://your-provider.example/v1", "your-key").await?;

// Both Codex and Grok default to device-code authorization.
let login = harness.login(Provider::Codex).await?;
println!("Open {} and enter {}", login["verificationUri"], login["userCode"]);

// The user can authorize from a browser on another device.
let status = harness.login_status().await?;
if status["state"] == "succeeded" {
    harness.switch_provider(Provider::Codex).await?;
}
# Ok(())
# }
```

Device authorization works on remote hosts without a local browser or callback
listener. `login` waits for the initial device-code request, then returns a
snapshot containing `verificationUri`, `userCode`, `authorizationUrl` (which may
include the code), and `expiresIn` seconds. Check `state` for a preparation
failure before displaying these fields. The native runtime polls for approval,
validates the account, and saves credentials in the instance profile.

ACP `fx/provider/login/start` also defaults to `"method":"device_code"`. It
returns immediately, possibly with `"state":"preparing"`; poll
`fx/provider/login/status` to obtain the code and observe completion. SDK and
ACP callers share the same login job, including cancellation. The private
device credential is never part of these snapshots. Final status remains
observable after completion.

To use a browser callback explicitly, call
`login_with_method(provider, harnel::LoginMethod::Browser)` or pass
`"method":"browser"` over ACP. Grok's browser flow additionally accepts a
manual callback code when `acceptsManualCode` is true. This is separate from
entering a device code on the provider website. The application controls
browser opening and display; the library never takes over its UI.

Other controls include `cancel_login`, `submit_login_code`, `provider_status`,
`provider_usage`, `refresh_credentials`, and `logout`. Refresh activates the
refreshed subscription provider. Logout removes its persisted credentials and
clears the active binding when applicable. Configured BYOK credentials survive
temporary provider switches.

`import_cli_credentials` and `credential_import_status` expose fx's existing
Codex/Grok CLI import flow. Imports use the instance profile and explicit
environment entries, rather than silently reading the application's default
credential directories.

## Add application tools

Implement `Tool::spec` and `Tool::execute`, then register the tool on the builder:

```rust,no_run
use harnel::{json, Result, tool::{BoxFuture, Tool, ToolCall, ToolOutput, ToolSpec}};

struct Inventory;
impl Tool for Inventory {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "inventory".into(),
            description: "Read the current inventory count".into(),
            parameters: json!({"type":"object","properties":{},"additionalProperties":false}),
            read_only: true,
        }
    }

    fn execute(&self, _call: ToolCall) -> BoxFuture<'_, Result<ToolOutput>> {
        Box::pin(async { Ok(ToolOutput::text("7 items available")) })
    }
}
```

Use `.tool(Inventory)` before `.build()`. Tools execute inside the native root
turn and pass through fx's existing admission policy. Native subagents currently
retain fx's built-in tool set. Mutable tools default to
requiring approval; the default SDK handler declines unresolved permission
requests. Configure `ClientHandler` to integrate application approvals. Use
`.native_tools(false)` to expose only application tools.

Tools must validate their own arguments. A JSON schema describes model input
but does not replace application validation. Cancelling a turn drops outstanding
host futures. Host callbacks must not detach untracked work or block the Tokio
executor. Built-in executor names cannot be replaced by application tools.

## Native build options

Applications will depend on `harnel`; the five platform crates are internal
implementation details. Until crates.io publication, a repository checkout
needs `python3 scripts/prepare-native.py` once to stage the pinned release asset.
Use `--target <rust-target>` when preparing a different compilation target.
This preparation step belongs to repository development and package assembly;
it is not required by consumers of the assembled crates.

To build the pinned fx source instead:

```sh
git submodule update --init --recursive
cargo build --no-default-features --features build-from-source
```

The source feature takes precedence if Cargo feature unification also enables
`prebuilt`; disable default features to avoid fetching the platform package.
`HARNEL_FX_SOURCE` selects an explicit fx source checkout when source building.
`HARNEL_FX_LIB_DIR` selects an explicit developer-owned static library directory;
use `--no-default-features` with this override to avoid the prebuilt dependency.
Its target and C ABI must match the SDK. A library built for another target or
CRT is not interchangeable, even when its exported C symbols have the same names.

## Control surface and boundaries

| Capability | Rust interface |
| --- | --- |
| Create, load, list, close, delete sessions | `session`, `load_session`, `list_sessions`, `Session::close/remove` |
| Attach to an ACP-created session | `session_handle` |
| Text, images, resources | `Session::ask/prompt/prompt_content` |
| Turn observation, steering, cancellation | `Session::status/steer/cancel` |
| Compaction | `Session::compact` |
| Model, reasoning effort, mode and session options | `Session::set_option/set_mode` |
| Provider, BYOK, OAuth, usage | Provider methods above |
| Tool projection | `bash_first`, `Builder::native_tools/tool` |
| Unified Exec input, output observation, termination | `Session::write_stdin/kill_process` |
| Native configuration overrides | `Builder::env/instructions` |
| Remaining native ACP extensions | `Harness::request/notify` |

`request` retains fx's native JSON shape and errors. It does not make an
unsupported native method available. Core features such as skills, permission
review, compaction, and subagents remain owned by fx, with its existing behavior
and provider limitations. MCP is intentionally unsupported in this fork.

Ordinary Unified Exec tools run directly in the embedded process. Legacy
terminal features that re-execute fx require an explicitly supplied
`FX_EMBEDDED_HELPER`; they never re-execute the embedding application. Custom
storage engines, a stable Rust hook API, and remote authenticated listeners are
outside this initial version.

Queues and frames are bounded. A slow event consumer receives `Error::Lagged`;
an ACP attachment that falls behind disconnects. Control calls time out after
60 seconds, host callbacks after five minutes. Dropping a prompt future cancels
its turn; other submitted controls may already have committed. Explicit
`shutdown` cancels work and waits for native cleanup. Final handle destruction
joins native workers before freeing state.

## Development and verification

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo doc --workspace --no-deps --locked
```

Integration tests use the actual native library and local protocol fixtures.
They cover model streaming, independent credentials, Rust tools, native shell
execution, shared ACP/SDK sessions, cancellation, BYOK persistence, and Codex/Grok
OAuth. The CI matrix runs the released native libraries on Linux and macOS for both
supported architectures, and on Windows x86_64 MSVC, without installing Zig or
checking out fx. Every runner executes the same SDK integration tests and CLI
workflow. Separate Linux and Windows jobs verify automatic compiler provisioning
and source builds. Linux x86_64 and Windows check the minimum Rust version.
The fx feature branch also runs its own Full CI and packaged C ABI smoke tests.

Release preparation and package verification:

```sh
python3 scripts/prepare-native.py --all
python3 scripts/bundle-native.py
python3 scripts/verify-packages.py
```

The verifier assembles all seven `.crate` files, checks package builds through a
local registry substitute, and runs an ordinary consumer and the packaged CLI
offline with Zig disabled. It writes the verified artifacts to
`target/verified-packages`. No command above publishes a crate. Publish the five
platform crates before `harnel-sys`, then `harnel`, only after release verification.

To update the engine release, update `crates/harnel-sys/native-release.json` with
the release tag and its exact revision, advance `vendor/fx` to that revision,
and run `python3 scripts/prepare-native.py --all --record`. Review the resulting
checksums and repeat package and platform CI before publication.

The workspace separates the safe SDK (`crates/harnel`), native ownership/build
boundary (`crates/harnel-sys`), and the independently versioned engine
(`vendor/fx`). Read [AGENTS.md](AGENTS.md) before contributing. Engine commits
must be published before the parent repository advances its submodule pointer.

## License

Apache-2.0. The bundled fx source retains its own copyright and license notices.
