#!/bin/sh
# Builds herdr for x86_64-unknown-linux-musl inside a throwaway rust container,
# mirroring .github/workflows/build-artifacts-manual.yml. Run through build.ps1.
# Mounts: /src = repo (read-only), /out = where the binary is written.
set -eu

apt-get update -qq
apt-get install -y -qq --no-install-recommends cmake ninja-build musl-tools xz-utils curl ca-certificates git >/dev/null

curl -fsSL https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz | tar -xJ -C /opt
export PATH="/opt/zig-x86_64-linux-0.16.0:$PATH"

# Build from a copy in the container filesystem: zig needs real symlinks, which a
# Windows bind mount cannot provide, and host build outputs must not leak in.
mkdir /build
tar -C /src --exclude=./target --exclude='*/.zig-cache' --exclude='*/zig-out' --exclude=./.zig-cache -cf - . | tar -C /build -xf -
cd /build

rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl

cp target/x86_64-unknown-linux-musl/release/herdr /out/herdr-x86_64-unknown-linux-musl
/out/herdr-x86_64-unknown-linux-musl --version
