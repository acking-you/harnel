# Initial implementation plan

The first usable version must demonstrate a real native agent turn, a Rust
host-tool call, shared SDK/ACP control, and provider authentication. A shell
wrapper around an installed agent is not an embedded implementation.

## Work sequence

1. Add a versioned native C ABI and an owned runtime transport to fx. Reuse its
   existing agent, provider, permission, and session implementations.
2. Build a safe Rust owner with typed commands, correlated responses, bounded
   event subscriptions, host effects, and explicit shutdown.
3. Attach stdio and listener ACP clients to that same owner. Keep connection
   identity and initialization separate from session/turn state.
4. Expose session, provider, OAuth, configuration, execution, and host-tool APIs.
5. Exercise complete deterministic workflows, then run native CI on Linux and
   macOS, for x86_64 and aarch64.
6. Document supported behavior and limitations, publish the Git repositories,
   and verify the exact revisions and CI outcomes.

## Release boundaries

The Git submodule is a development input, not a dependency that crates.io users
can be expected to initialize. Distribution must provide self-contained native
inputs. Source builds and prebuilt artifacts must identify the fx revision and
ABI version. This initial repository work does not authorize a crates.io release.

The acceptance test for shared state is: create a session through the SDK,
attach an ACP client, start a real turn, observe its events through both paths,
steer or cancel it through ACP, and continue using the session through the SDK
after the ACP client disconnects.
