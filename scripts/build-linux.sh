#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_HOME="$repo_root/.cargo"
export RUSTUP_HOME="$repo_root/.rustup"
export PATH="$CARGO_HOME/bin:$PATH"

case "$(uname -m)" in
  x86_64 | amd64)
    rust_host="x86_64-unknown-linux-gnu"
    ;;
  aarch64 | arm64)
    rust_host="aarch64-unknown-linux-gnu"
    ;;
  *)
    echo "Unsupported Linux architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

toolchain="stable-$rust_host"

if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
  echo "Repo-local Rust missing. Run scripts/bootstrap-linux-rust.sh first." >&2
  exit 1
fi

cd "$repo_root"
rustup run "$toolchain" cargo build --release
