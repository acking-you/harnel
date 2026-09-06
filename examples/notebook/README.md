# Harnel Notebook

A small, complete Markdown notebook assistant. Ask a question about your notes;
the embedded agent calls a read-only Rust tool, reads the requested note, and
streams an answer with filename citations. Saved conversations survive process
restarts. An optional ACP listener exposes the same session while a turn runs.

This is an independent Cargo application. Its manifest uses
`harnel = "0.1.0"` from crates.io, with no path dependency, patch, fx executable,
or Git submodule. Copy this directory anywhere and run it with Rust 1.85 or
newer and your platform's normal Rust linker. Harnel's default dependency
includes the matching precompiled native library; Zig is unnecessary.

## Run

```sh
export HARNEL_BASE_URL='https://your-provider.example/v1'
export HARNEL_API_KEY='your-key'
export HARNEL_MODEL='your-provider-model-id'
cargo run -- notes "When does Atlas launch, and who owns the checklist?"
```

The endpoint must support OpenAI Responses streaming and function calling.
Choose a model offered by that endpoint. In PowerShell, set variables with
`$env:HARNEL_API_KEY = 'your-key'` and similarly for the URL and model.

The bundled `notes/launch.md` and `notes/support.md` are ordinary editable files.
Pass your own notes directory as the first argument. Stdout contains the answer;
stderr contains the session ID, native revision, and stop reason.

## Continue later

```sh
# Copy the Session ID from the previous run:
cargo run -- notes "Who should handle urgent incidents?" 'the-session-id'
```

The application saves state under `notes/.harnel-state/.fx/` by default. Set
`HARNEL_STATE_DIR` to choose another profile. Use the same notes directory and
profile when resuming. Omit the session ID to start a new conversation.

```sh
export HARNEL_LISTEN='127.0.0.1:7788'
cargo run -- notes "Compare the launch and support notes."
```

While the turn runs, an ACP client can initialize and attach to the printed
session ID through the listener. A stdio client can use
`harnel acp --connect 127.0.0.1:7788`. The listener closes when the answer finishes.
It grants trusted local clients control of the same engine. Do not create a
second session if you intend to observe the SDK's current session.

## Application boundary

`src/notebook.rs` loads a snapshot of top-level Markdown files at startup, with
at most 64 files, 64 KiB per file, and 1 MiB overall. Note IDs are exact filenames
from that snapshot. Tool arguments never become filesystem paths. Subdirectories
and symbolic links are not loaded. Restart the application to pick up edits.

The model sees available filenames and receives note contents only after
calling `read_note`. Built-in shell and filesystem tools are disabled. The host
provides the data and tool; Harnel owns the model loop, tool dispatch, streaming,
and saved conversation. Only run against a provider trusted to receive your notes.

## Deterministic integration check

From a Harnel checkout, after building this application:

```sh
cargo build --manifest-path examples/notebook/Cargo.toml
python3 scripts/smoke-examples.py --notebook examples/notebook/target/debug/harnel-notebook
```

Use the `.exe` suffix on Windows. The check supplies a local Responses server,
creates an actual Markdown file, verifies that the model receives the Rust tool's
contents, checks the streamed answer, and restarts the application with the saved
session. It needs no model credentials. This verifies the full native integration;
use the Run instructions above to evaluate answers from a real model.
