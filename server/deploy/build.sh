#!/bin/bash
set -euo pipefail
umask 022
base=/opt/harness-scope
export RUSTUP_HOME="$base/build-tools/rustup"
export CARGO_HOME="$base/build-tools/cargo"
export CARGO_TARGET_DIR="$base/build-target"
export CARGO_BUILD_JOBS=1
export PATH="$CARGO_HOME/bin:$PATH"
mkdir -p "$base/build-tools"
if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
    cd "$base/build-tools"
    url=https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu
    curl --fail --location --retry 3 --output rustup-init "$url/rustup-init"
    curl --fail --location --retry 3 --output rustup-init.sha256 "$url/rustup-init.sha256"
    sha256sum --check rustup-init.sha256
    chmod 0755 rustup-init
    ./rustup-init -y --no-modify-path --profile minimal --default-toolchain 1.91.0
fi
cd "$base/src"
cargo +1.91.0 build --release --locked -p harness-scope-service
echo BUILD_COMPLETE
