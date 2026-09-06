# harnel-native-aarch64-unknown-linux-gnu

Precompiled static fx runtime for `aarch64-unknown-linux-gnu`. This is an internal dependency of
Harnel; applications should depend on `harnel` directly. The package contains
the exact library, header, license notices, and manifest from a pinned fx
release. The build script checks its target, ABI, and checksum without network
access or a Zig installation.
