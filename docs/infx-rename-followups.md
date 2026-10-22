# INFX rename and publication

## Completed rename

- The GitHub repository is `acking-you/infx`. GitHub redirects the former
  `acking-you/harnel` URL. Existing clones can update their remote with
  `git remote set-url origin https://github.com/acking-you/infx.git`.
- Crates, directories, the CLI, environment variables, ACP metadata, scripts,
  CI, and public links use `infx`.
- The fx source sends private host tool callbacks as `_infx/tool/call`.
  `infx_sys::is_host_tool_method` also accepts the former method from the
  immutable fx v0.0.10 prebuilt libraries. Both names are blocked on external
  ACP connections. This compatibility is intentional, not an unfinished
  textual rename.
- The README and notebook guide explain how to keep using an existing
  `.harnel-state` profile with `INFX_STATE_DIR`.

The release lock still pins the original fx v0.0.10 binaries and checksums.
The submodule and bundled source include the callback rename; the SDK supports
both engine versions. Do not edit release checksums or remove legacy callback
support without staging and testing a newer native release.

## Publication

The seven `infx` 0.1.0 crates were published to crates.io on October 22, 2026,
with maintainer authorization, from revision `5f10bc2`. That revision pins the
fx submodule at `76e507a6` and the prebuilt fx v0.0.10 libraries at `8d7653db`.

- fx PR acking-you/fx#46 passed Full CI (run 39784091074) and was merged into
  `byok` before publication, so the gitlink references a published commit.
- Parent CI passed on all five targets for that exact revision, in prebuilt,
  source, and packaged-consumer jobs (run 39792712979). The macOS aarch64
  prebuilt job passed on its second attempt; see the native shell item below.
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

## Next native release

These items are planned; none is implemented yet. They all require a new fx
release staged for all five targets. Because 0.1.0 is published, increment the
Rust package versions for that release.

- **Retire the former callback.** When an fx release containing
  `_infx/tool/call` is available, update `crates/infx-sys/native-release.json`
  with its real tag and revision, advance the submodule, and stage the checked
  assets with `python3 scripts/prepare-native.py --all --record`. Update the
  documented fx version. Only then remove the former method from the native
  adapter and adjust its ACP isolation test.
- **Align macOS archive members.** In the fx v0.0.10 macOS `libfx_core.a`, the
  `libfx_core_zcu.o` member data starts at an offset that is not 8-byte
  aligned. The Xcode 27 linker ignores that member when it links the archive
  directly, so `cargo test -p infx-sys` fails to link with Xcode 27. Dependent
  crates link correctly because rustc re-archives the bundled objects, which
  `verify-registry.py` confirmed with Xcode 27. Produce aligned archives and
  link `infx-sys` tests with Xcode 27 before recording the release.
- **Investigate a native shell failure.** `native_shell_executes_without_an_fx_process`
  failed once on the macOS aarch64 prebuilt job (run 39792712979, attempt 1):
  the turn returned JSON-RPC `-32603` with message `Unexpected`, meaning a
  system call's unexpected errno escaped the fx prompt worker. It passed on
  rerun and in 400 local runs on aarch64 macOS. Identify the failing call in
  fx and report such failures as a tool error instead of failing the turn.

Run the same source, prebuilt, package, and registry checks for that release.
