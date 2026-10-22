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

## Remaining publication

The maintainer authorized publishing the seven `infx` 0.1.0 crates on
October 22, 2026, after successful verification. The README and notebook guide
now describe the released crates; the README embedded in a published crate
cannot be updated without another crate release. The old `harnel` release
remains available to existing users and is not yanked.

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

### 2. Publish 0.1.0 in dependency order

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

### 3. Verify registry consumers

```sh
python3 scripts/verify-registry.py
```

Dispatch **Published crate verification**, which runs all five target platforms
with fresh Cargo caches and real crates.io dependencies. Only after it passes,
record publication as verified in this checklist.

Keep the old `harnel` crate available. An optional later maintenance release can
point its users to `infx`; yanking it is not required for the rename and should
be a separate maintainer decision.

## Retiring native compatibility later

When an fx release containing `_infx/tool/call` is available on all five native
targets, update `crates/infx-sys/native-release.json` with its real tag and
revision, advance the submodule, and stage the checked assets with
`python3 scripts/prepare-native.py --all --record`. Update the documented fx
version and increment the Rust package versions if 0.1.0 has been published.
Only then remove the former method from the native adapter and adjust its ACP
isolation test. Run the same source, prebuilt, package, and registry checks.
