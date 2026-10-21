# infx-native-x86_64-unknown-linux-gnu

Precompiled static fx runtime for `x86_64-unknown-linux-gnu`. This is an internal dependency of
INFX; applications should depend on `infx` directly. The package contains
the exact library, header, license notices, and manifest from a pinned fx
release. The build script checks its target, ABI, and checksum without network
access or a Zig installation.
