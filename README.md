# Harnel

An embeddable agent harness for Rust, powered by [fx](https://github.com/acking-you/fx).

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

## Status

Initial implementation is in progress. Harnel has not been published to
crates.io. Supported APIs, build instructions, and verification results will be
documented alongside the implementation; the vision above is not a claim that
every capability is already implemented.

## Design commitments

- A real native library that runs inside the host application.
- One owner for execution state, shared by SDK and ACP attachments.
- First-class BYOK configuration, provider switching, and Codex/Grok OAuth.
- Host-defined Rust tools and explicit application configuration.
- Bounded queues, predictable cancellation, and deterministic shutdown.
- English documentation and reproducible, cross-platform CI.

## License

Apache-2.0. The bundled fx source retains its own copyright and license notices.
