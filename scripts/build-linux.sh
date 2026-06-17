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

if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
  echo "Repo-local Rust missing. Run scripts/bootstrap-linux-rust.sh first." >&2
  exit 1
fi

if ! command -v musl-gcc >/dev/null 2>&1; then
  echo "musl-gcc missing. Install it first: sudo apt install -y musl-tools" >&2
  exit 1
fi

case "$linux_target" in
  x86_64-unknown-linux-musl)
    export CC_x86_64_unknown_linux_musl="${CC_x86_64_unknown_linux_musl:-musl-gcc}"
    ;;
  aarch64-unknown-linux-musl)
    export CC_aarch64_unknown_linux_musl="${CC_aarch64_unknown_linux_musl:-musl-gcc}"
    ;;
esac

cd "$repo_root"
rustup run "$toolchain" cargo build --release --target "$linux_target" --bin mafia
