#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_HOME="$repo_root/.cargo"
export RUSTUP_HOME="$repo_root/.rustup"
export PATH="$CARGO_HOME/bin:$PATH"

case "$(uname -m)" in
  x86_64 | amd64)
    rust_host="x86_64-unknown-linux-gnu"
    linux_target="x86_64-unknown-linux-musl"
    ;;
  aarch64 | arm64)
    rust_host="aarch64-unknown-linux-gnu"
    linux_target="aarch64-unknown-linux-musl"
    ;;
  *)
    echo "Unsupported Linux architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

toolchain="stable-$rust_host"

mkdir -p "$CARGO_HOME" "$RUSTUP_HOME"

if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-host "$rust_host" --default-toolchain stable --no-modify-path
fi

rustup set default-host "$rust_host"
rustup toolchain install "$toolchain" --profile minimal
rustup default "$toolchain"
rustup target add "$linux_target" --toolchain "$toolchain"
rustup component add clippy rustfmt --toolchain "$toolchain"
rustup run "$toolchain" rustc -V
rustup run "$toolchain" cargo -V
