# Runnable SDK examples

These applications use the native fx engine inside their Rust process. They
run from a Harnel checkout and are also included in the `harnel` crate package.
For a separate application with an ordinary crates.io dependency, see the
[notebook assistant](../../../examples/notebook/README.md).

## Setup

From the repository root:

```sh
python3 scripts/prepare-native.py
export HARNEL_BASE_URL='https://your-provider.example/v1'
export HARNEL_API_KEY='your-key'
export HARNEL_MODEL='your-provider-model-id'
```

Use an endpoint implementing the OpenAI Responses protocol, including streaming
and function calling for tool examples. A Chat Completions-only endpoint does
not work. `HARNEL_MODEL` must name a model available at that endpoint.

PowerShell uses environment assignments such as:

```powershell
python scripts/prepare-native.py
$env:HARNEL_BASE_URL = 'https://your-provider.example/v1'
$env:HARNEL_API_KEY = 'your-key'
$env:HARNEL_MODEL = 'your-provider-model-id'
```

Examples disable built-in filesystem and shell tools with `native_tools(false)`.
Only `custom_tool` registers an application tool. All examples explicitly shut
down their harness after success or a request failure. No example prints API keys.

## Choose an example

| Example | Run from the repository root | What to inspect |
| --- | --- | --- |
| [embed](embed.rs) | `cargo run -p harnel --example embed -- "Say hello"` | Builder, saved session handle, collected answer, shutdown |
| [stream](stream.rs) | `cargo run -p harnel --example stream -- "Explain Rust ownership"` | Subscribe before prompting, flush chunks, drain final events, Ctrl-C cancellation |
| [custom_tool](custom_tool.rs) | `cargo run -p harnel --example custom_tool` | JSON schema, argument validation, read-only Rust callback, model/tool round trip |
| [sessions](sessions.rs) | `cargo run -p harnel --example sessions` | Persistent profile, session ID, loading, closing without deletion |
| [provider](provider.rs) | `cargo run -p harnel --example provider` | Persist and activate BYOK credentials, provider switching, shared SDK clone |
| [oauth](oauth.rs) | `cargo run -p harnel --example oauth -- codex` | Device authorization, terminal states, expiration, cancellation, saved credentials |
| [acp](acp.rs) | `cargo run -p harnel --example acp` | SDK-created session shared with stdio and listener, independent detach |

## Streaming and cancellation

`stream` prints assistant text to stdout and tool progress to stderr. Press
Ctrl-C while the model is responding. The example sends `Session::cancel` and
continues draining events until the native turn finishes. Windows also accepts
Ctrl-Break. Dropping a prompt future cancels its turn too, but waiting for the
reply gives the application an explicit stop reason.

Subscribers use bounded queues. This example returns an error if it falls
behind; a production UI may catch `Error::Lagged`, mark the transcript as
incomplete, and continue receiving. A completed prompt can become ready while
its final chunks are still queued, so always drain pending events before
finishing the display.

## Application tools and permissions

`custom_tool` owns an in-memory inventory with seven keyboards and three
monitors. The model calls `lookup_inventory`; Rust validates the SKU, looks up
the value, and returns JSON. The native engine sends that result back to the
model and produces the final answer. There is no second agent loop in Rust.

`read_only` must describe the implementation honestly. A mutating tool should
set it to `false` and use a `ClientHandler` that forwards
`session/request_permission` to the application's approval UI. The default
handler declines unresolved approvals. Return an ACP outcome with the exact
option ID offered in the request, for example:

```rust
harnel::json!({"outcome":{"outcome":"selected","optionId":selected_option_id}})
```

The UI must actually obtain that selection; do not automatically select an
allow option merely because a tool asked for permission. ACP-initiated turns
send these requests to their originating ACP client. Tool futures should be
cancellation-safe and must not detach untracked side effects.

## Continue a conversation in another process

```sh
export HARNEL_STATE_DIR="$PWD/agent-state"
cargo run -p harnel --example sessions -- "Remember that my project is named Atlas."
# Copy the Session ID printed to stderr:
export HARNEL_SESSION_ID='the-session-id'
cargo run -p harnel --example sessions -- "What is my project called?"
```

The second process loads the saved conversation from the same profile. Session
state lives under `<HARNEL_STATE_DIR>/.fx/sessions/`. `close` unloads a session;
`remove` deletes it. Without `state_dir`, the harness creates a temporary profile.
Use separate harnesses and profiles for independent concurrent root sessions.

## Persist BYOK configuration and use device login

`provider` requires `HARNEL_STATE_DIR` and the three provider variables from
Setup. `configure_provider` validates and persists the URL/key pair, then
activates Gateway. A cloned SDK handle sees that same binding. After a Codex or
Grok login, `switch_provider(Provider::Codex)` or `Provider::Grok` activates it;
switching back to Gateway restores the persisted BYOK binding.

OAuth needs only a persistent profile and a provider name:

```sh
export HARNEL_STATE_DIR="$PWD/agent-state"
cargo run -p harnel --example oauth -- codex
# Or:
cargo run -p harnel --example oauth -- grok
```

Open the displayed URL on any device and enter the displayed user code. The
library polls and saves the resulting credentials. You do not paste that user
code back into this process. Ctrl-C cancels the shared login job. To run a turn
after login, build with the same `state_dir`, select the corresponding
`Provider`, and choose a model offered by the account. BYOK environment
variables are unnecessary for a subscription provider.

Device-code examples contact the actual provider and require a subscription
account. CI exercises both login flows with deterministic OAuth fixtures in
`tests/auth.rs`; it never authorizes a real account automatically.

## Attach ACP clients to the SDK's session

```sh
export HARNEL_LISTEN='127.0.0.1:7788'
cargo run -p harnel --example acp
```

The example creates a session through Rust and prints its ID to stderr. Both
stdin/stdout and the listener attach to that engine. A stdio-only ACP client
can connect through `harnel acp --connect 127.0.0.1:7788`. Each client must send
ACP `initialize`, then it can use the printed session ID. Creating or loading
another session changes the shared selection for every attachment.

Stdout is reserved for newline-delimited JSON-RPC. Closing stdin detaches that
client while the listener remains active. Ctrl-C shuts down both transports
and the engine. The listener accepts loopback addresses only and is a trusted
local control interface, without per-client authentication.

## Run without model credentials

```sh
cargo build -p harnel --examples --locked
python3 scripts/smoke-examples.py --examples target/debug/examples
```

This executes the real applications and native library against a local scripted
Responses server. It checks collected and streamed answers, cancellation,
inventory callbacks, provider activation, conversation persistence across
processes, and shared ACP attachments. It tests integration behavior, not the
quality or availability of a hosted model. The same checks run on all five
supported native targets, including Windows.
