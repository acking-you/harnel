# harnel-native-x86_64-pc-windows-msvc

Precompiled static fx runtime for `x86_64-pc-windows-msvc`. This is an internal dependency of
Harnel; applications should depend on `harnel` directly. The package contains
the exact library, header, license notices, and manifest from a pinned fx
release. The build script checks its target, ABI, and checksum without network
access or a Zig installation.
