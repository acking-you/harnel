# harnel-sys

Native fx ownership and build integration for [Harnel](https://github.com/acking-you/harnel).
Applications normally depend on `harnel` instead of this crate.

Requires Zig 0.16.0 and a platform C linker. Cargo builds a static native library
for Linux GNU or macOS on x86_64/aarch64, or Windows MSVC on x86_64. Windows
requires Visual Studio C++ Build Tools and the Windows SDK.
Repository builds use the fx submodule;
packaged builds use the included native source and its exact `REVISION`.
The ABI wrapper owns every handle and joins native workers before destruction.

`ZIG` selects the compiler executable. `HARNEL_FX_SOURCE` selects a development
source checkout. `HARNEL_FX_LIB_DIR` explicitly links a developer-supplied
`libfx_core.a` (`fx_core.lib` for MSVC); its ABI and target must match Cargo's target.
No native library or compiler is downloaded by the build script.
