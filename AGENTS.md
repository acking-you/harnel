# Agent guidance

## Product contract

Harnel is an embeddable Rust agent harness backed by the fx Zig runtime. Its
purpose is to make an agent as easy to embed as a database. Keep the public API
small, typed, and useful without requiring a separate fx installation.

All documentation, code comments, diagnostics, and commit descriptions must be
written in English. Document actual behavior separately from planned work.

## Ownership

- `crates/harnel` owns safe Rust APIs, shared control, and ACP attachments.
- `crates/harnel-sys` owns the native ABI declarations and build integration.
- `vendor/fx` is an independent Git submodule. Follow its nearest `AGENTS.md`.
  Engine behavior belongs there, including the agent loop, tools, providers,
  authentication, compaction, and durable session semantics.
- `examples` contains runnable applications, not alternate implementations.
- `crates/harnel-native-*` own target-specific release payloads. They do not
  compile fx or download artifacts in consumer builds.
- Build and release automation belongs in `xtask` or `scripts`.

Do not duplicate the agent loop in Rust, execute an installed fx binary as an
implicit library backend, or mutate the host process environment to configure
an instance. Preserve unrelated user work and the parent/submodule boundary.

## Shared state and lifecycle

SDK and ACP callers must operate on the same session and turn owners. A
connection owns transport state, request IDs, capabilities, and subscriptions;
it does not own the lifetime of an embedded harness.

Serialize competing changes to one session. Keep network I/O and host callbacks
outside control locks. Use bounded queues and explicit lag/overflow outcomes.
Scope correlation IDs to their owner. Route replies to their origin and
broadcast only public events to authorized subscribers.

Native handles and returned memory need documented ownership. Keep `unsafe`
code in the narrowest possible modules, document its invariants, and never
allow a Rust panic to cross the C ABI. Shutdown must wake blocked operations,
settle pending work, and join workers before freeing their state.

Credentials, provider endpoints, and account identity form one binding. Preserve
the route captured by an active turn. OAuth and provider jobs belong to the
runtime and must be observable from both SDK and ACP.

## Verification

Run focused tests while developing. Before reporting a feature as working:

1. Run formatting, compilation, and Clippy with warnings denied.
2. Run the relevant integration tests against the native fx library.
3. Run a real example or CLI interaction through the changed public API.
4. Verify CI for the exact pushed revision on every supported target.

The native CI matrix includes Linux GNU and macOS on x86_64/aarch64 and Windows
x86_64 MSVC. Run the same SDK, OAuth, ACP, and CLI workflows on Windows. Keep
process control and test fixtures portable; do not skip runtime tests merely
because Unix signals, pipe polling, or command names differ.

Test meaningful boundaries: native lifecycle, shared SDK/ACP state, concurrent
requests, cancellation, host tools, provider controls, framing, and queue limits.
Use deterministic local model/OAuth fixtures for CI. Keep credentialed live
checks opt-in and never print credentials.

Publish a submodule commit before pushing a parent gitlink that references it.
Verify remote revisions after pushes. Do not publish to crates.io or claim a
release without explicit authorization and a verified package build.

## Native distribution

Default consumer builds must link the pinned release archive without Zig,
submodule initialization, or build-script network access. Keep release tag,
revision, ABI, target, and checksums explicit in the release lock. Stage payloads
through `scripts/prepare-native.py`; never commit generated library archives.

Keep source builds explicit and test compiler provisioning independently.
`HARNEL_OFFLINE` must prevent automatic compiler downloads. Do not mutate the
host PATH or install compilers globally. Before publication, run
`scripts/verify-packages.py` and the no-Zig native matrix on the exact revision.
The registry dependency order is platform crates, harnel-sys, then harnel.
