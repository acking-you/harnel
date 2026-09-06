# harnel-sys

Native fx ownership and build integration for [Harnel](https://github.com/acking-you/harnel).
Applications normally depend on `harnel` instead of this crate.

Default builds statically link a pinned fx v0.0.8 release from a target-specific
platform crate. Linux GNU and macOS support x86_64/aarch64; Windows supports
x86_64 MSVC. Consumers need their normal Rust linker and platform SDK, with no
Zig installation or build-script network access. Platform packages validate
the native archive checksum, target, and ABI; this crate also checks the revision.

Select `--no-default-features --features build-from-source` for a source build.
Repository builds use the fx submodule; packaged builds use the included source
and its `REVISION`. Zig 0.16.0 is acquired and checksum-verified automatically
inside Cargo's build output directory when needed. `ZIG` selects an existing
compiler; `HARNEL_OFFLINE=1` disables compiler downloads. Automatic provisioning
selects the compiler for Cargo's host, while native compilation uses its target.

`HARNEL_FX_SOURCE` selects an explicit source checkout for source builds.
`HARNEL_FX_LIB_DIR` explicitly links a developer-supplied `libfx_core.a`
(`fx_core.lib` for MSVC); disable default features with this override to avoid
fetching the platform crate. Its ABI, target, and CRT must match the application.

The ABI wrapper owns every handle and joins native workers before destruction.
