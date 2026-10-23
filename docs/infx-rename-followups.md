# INFX rename and publication

## Completed rename

- The GitHub repository is `acking-you/infx`. GitHub redirects the former
  `acking-you/harnel` URL. Existing clones can update their remote with
  `git remote set-url origin https://github.com/acking-you/infx.git`.
- Crates, directories, the CLI, environment variables, ACP metadata, scripts,
  CI, and public links use `infx`.
- fx sends private host tool callbacks as `_infx/tool/call`, which is blocked
  on external ACP connections. INFX 0.1.0 also accepted the former
  `_harnel/tool/call` from the fx v0.0.10 prebuilt libraries; the repository
  removed that compatibility when it moved to fx v0.0.11.
- The README and notebook guide explain how to keep using an existing
  `.harnel-state` profile with `INFX_STATE_DIR`.

The repository's release lock and submodule pin fx v0.0.11 (revision
`baa53168`); see **fx v0.0.11 native release** below. The published 0.1.0
crates still contain fx v0.0.10.

## Publication

The seven `infx` 0.1.0 crates were published to crates.io on October 22, 2026,
with maintainer authorization, from revision `5f10bc2`. That revision pins the
fx submodule at `76e507a6` and the prebuilt fx v0.0.10 libraries at `8d7653db`.

- fx PR acking-you/fx#46 passed Full CI (run 39784091074) and was merged into
  `byok` before publication, so the gitlink references a published commit.
- Parent CI passed on all five targets for that exact revision, in prebuilt,
  source, and packaged-consumer jobs (run 39792712979). The macOS aarch64
  prebuilt job passed on its second attempt; see **fx v0.0.11 native release**.
- `verify-packages.py` passed on that revision before publication.
- After publication, `verify-registry.py` passed on aarch64 macOS with the
  Xcode 27 linker, and **Published crate verification** passed on all five
  targets (run 39799446446).

The README and notebook guide describe the released crates; the README
embedded in a published crate cannot change without another crate release.
The old `harnel` release remains available to existing users and is not
yanked. An optional later maintenance release can point its users to `infx`;
yanking it should be a separate maintainer decision.

## Release procedure

### 1. Verify the exact release revision

Publish the fx source commit before a parent gitlink references it. Require the
parent's CI to pass on Linux GNU and macOS (x86_64/aarch64) and Windows x86_64
MSVC, including both prebuilt and source build jobs and package verification.
The fx change has its own Full CI requirement.

Prepare and verify all payloads from that same parent revision:

```sh
git submodule update --init --recursive
python3 scripts/prepare-native.py --all
python3 scripts/bundle-native.py
python3 scripts/verify-packages.py
```

If `crates/infx-sys/native` already exists, remove only that ignored generated
bundle before running `bundle-native.py` again. The bundle must come from a
clean, committed submodule. The verifier writes seven checked `.crate` files
to `target/verified-packages` and exercises the SDK, CLI, examples, and notebook
through an offline registry substitute, without Zig.

### 2. Publish in dependency order

After authorization and successful CI, use a crates.io token with publish scope:

```sh
cargo login

for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
              x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc; do
  cargo publish --manifest-path "crates/infx-native-$target/Cargo.toml" \
    --allow-dirty --no-verify
done
cargo publish -p infx-sys --allow-dirty
cargo publish -p infx --allow-dirty
```

Packaging includes generated, ignored payloads, hence `--allow-dirty`; tracked
sources must still match the verified revision. Platform packages use
`--no-verify` because their build scripts accept only their own target.
`verify-packages.py` assembles all five, and the native CI matrix verifies each
on its target. `infx-sys` and `infx` are verified again by `cargo publish`.

`cargo publish` verifies `infx-sys` and `infx` against the platform crates
published moments earlier. Cargo reads `.cargo/config.toml` from every ancestor
of the working directory as well as `CARGO_HOME`, so a crates.io mirror in that
configuration can hide them. In that case, publish from a detached worktree of
the verified revision outside those directories, with the verified generated
payloads copied in and a clean `CARGO_HOME`. crates.io rate-limits new crates:
a first release of all seven waits about ten minutes before each of the last
two uploads.

### 3. Verify registry consumers

```sh
python3 scripts/verify-registry.py
```

Dispatch **Published crate verification**, which runs all five target platforms
with fresh Cargo caches and real crates.io dependencies. Only after it passes,
record publication as verified in this checklist.

## fx v0.0.11 native release

The planned native release is staged in the repository; INFX 0.1.1 is not
published.

- **Native shell failure.** The `-32603 Unexpected` in run 39792712979 was not
  a shell failure. A diagnostic build reproduced it under compile-time CPU
  contention, also in tests that never run a shell. Zig 0.16's POSIX
  `connect` retries `connect(2)` after `EINTR`; a stray `SIGIO` from
  `std.Io.Threaded` cancellation interrupted a loopback connection that then
  completed, so the retry failed with `EISCONN`, which release builds report
  as `error.Unexpected`. fx treated that request-open failure as terminal.
  acking-you/fx#48 reports it as the retryable `ConnectionSetupFailed`, so the
  agent retries the model request.
- **Spurious bounded deadlines.** Pre-release verification found a second
  failure from the same signals: `Event.waitTimeout` reports any wakeup as
  `Timeout`, so OAuth device-code polls failed with `Timeout` about one second
  into `tests/auth.rs` (14 of 480 stressed runs). acking-you/fx#49 re-checks
  the clock before a deadline wins; the same stress then passed 480 of 480.
- **Aligned macOS archives.** Zig's archiver aligned members to two bytes.
  acking-you/fx#47 re-archives Darwin `libfx` with `zig ar --format=darwin`,
  and `package-native.py` rejects unaligned members. `cargo test -p infx-sys`
  now links with the Xcode 27 linker (ld-27037); with v0.0.10 it fails.
- **Release.** Each fx PR passed Full CI on its exact head (runs 39917056357,
  39920411373, 39928061870) and release PR acking-you/fx#50 passed run
  39928355743. Release run 39942494600 tagged `v0.0.11` at `baa53168` and
  published the CLI archives and all five native libraries with checksum files.
- **INFX.** `prepare-native.py --all --record` staged and recorded all five
  targets; the submodule points to `baa53168`. The SDK no longer accepts
  `_harnel/tool/call`, and the ACP isolation test covers `_infx/tool/call`.
  The Rust packages are version 0.1.1.

Before publishing 0.1.1, follow **Release procedure** on the exact parent
revision: CI on all five targets, `verify-packages.py`, publication with
maintainer authorization, then `verify-registry.py` and **Published crate
verification**.
