# INFX rename: remaining release steps

This checklist records the work that remains after the repository was renamed
from Harnel to INFX. Items below are planned maintainer actions; none of them
has been performed yet.

## Current state

- Crates, directories, the CLI, environment variables, scripts, CI, and
  documentation use the `infx` name.
- Local verification and package verification passed for the rename commit,
  and the native CI matrix passed on Linux GNU and macOS (x86_64/aarch64) and
  Windows x86_64 MSVC.
- `harnel` 0.1.0 is the only version published to crates.io. No `infx` crate
  has been published.
- The pinned fx v0.0.10 release still sends its private host tool request as
  `_harnel/tool/call`. The SDK keeps that engine-owned name in one constant,
  `infx_sys::HOST_TOOL_METHOD`.

## 1. Rename the GitHub repository

Rename `acking-you/harnel` to `acking-you/infx` in the repository settings.
Manifests, README links, the CI badge, and the README poster already point at
`acking-you/infx`. The poster uses an absolute `main` branch URL so that it also
renders on crates.io; it appears once the rename is done.

Update existing clones:

```sh
git remote set-url origin https://github.com/acking-you/infx.git
```

## 2. Publish the crates

Publish from the exact revision whose CI passed. Packaging needs the staged
native payloads and the bundled fx source, which are generated and ignored by
Git, hence `--allow-dirty`.

```sh
git submodule update --init --recursive
python3 scripts/prepare-native.py --all
python3 scripts/bundle-native.py
python3 scripts/verify-packages.py

cargo login   # use a crates.io token with publish scope

for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
              x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc; do
  cargo publish --manifest-path "crates/infx-native-$target/Cargo.toml" \
    --allow-dirty --no-verify
done
cargo publish -p infx-sys --allow-dirty
cargo publish -p infx --allow-dirty
```

The platform crates use `--no-verify` because each build script only accepts
its own target; `verify-packages.py` already checked them. `infx-sys` and `infx`
are verified by `cargo publish` against the platform crate for the host.

## 3. Verify the published crates

```sh
python3 scripts/verify-registry.py
```

Then dispatch the **Published crate verification** workflow, which repeats the
check on all five release targets with a fresh Cargo cache and real crates.io
dependencies.

## 4. Point `harnel` users to `infx`

Optional. Choose one:

- Publish a final `harnel` release whose README and description say the crate
  moved to `infx`. This needs the previous `harnel` sources, because this
  repository no longer builds that crate.
- Yank `harnel` 0.1.0 with `cargo yank --version 0.1.0 harnel`. Existing
  lockfiles keep working; new projects cannot select it.

## 5. Remove the last engine-owned Harnel name

1. In `acking-you/fx`, rename the host tool request in `src/acp/prompt.zig`
   and `docs/embedding.md` from `_harnel/tool/call` to `_infx/tool/call`, then
   release it through the fx process (for example v0.0.11).
2. Here, update `crates/infx-sys/native-release.json` with the new tag and
   revision, advance `vendor/fx` to that revision, and run
   `python3 scripts/prepare-native.py --all --record`.
3. Set `infx_sys::HOST_TOOL_METHOD` to `"_infx/tool/call"` and update the
   README's fx version.
4. Bump the workspace and platform crate versions (for example 0.1.1), rerun
   the full verification and CI matrix, and publish in the order above.
