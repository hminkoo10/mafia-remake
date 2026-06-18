#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_HOME="$repo_root/.cargo"
export RUSTUP_HOME="$repo_root/.rustup"
export PATH="$CARGO_HOME/bin:$PATH"

linkage="${1:-glibc}"

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

case "$linkage" in
  musl)
    linux_target="aarch64-unknown-linux-musl"
    ;;
  glibc | gnu)
    linux_target="aarch64-unknown-linux-gnu"
    ;;
  *)
    echo "Usage: $0 [musl|glibc]" >&2
    exit 1
    ;;
esac

toolchain="stable-$rust_host"

if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
  echo "Repo-local Rust missing. Run scripts/bootstrap-linux-rust.sh first." >&2
  exit 1
fi

rustup target add "$linux_target" --toolchain "$toolchain"

case "$linux_target" in
  aarch64-unknown-linux-musl)
    if command -v aarch64-linux-musl-gcc >/dev/null 2>&1; then
      export CC_aarch64_unknown_linux_musl="${CC_aarch64_unknown_linux_musl:-aarch64-linux-musl-gcc}"
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER:-aarch64-linux-musl-gcc}"
    elif [ "$(uname -m)" = "aarch64" ] || [ "$(uname -m)" = "arm64" ]; then
      if ! command -v musl-gcc >/dev/null 2>&1; then
        echo "musl-gcc missing. Install it first: sudo apt install -y musl-tools" >&2
        exit 1
      fi
      export CC_aarch64_unknown_linux_musl="${CC_aarch64_unknown_linux_musl:-musl-gcc}"
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER:-musl-gcc}"
    else
      echo "aarch64 musl cross compiler missing. Put aarch64-linux-musl-gcc in PATH, or build on an aarch64 Linux host with musl-tools." >&2
      exit 1
    fi
    ;;
  aarch64-unknown-linux-gnu)
    if [ "$(uname -m)" != "aarch64" ] && [ "$(uname -m)" != "arm64" ]; then
      if ! command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
        echo "aarch64 glibc cross compiler missing. Install it first: sudo apt install -y gcc-aarch64-linux-gnu" >&2
        exit 1
      fi
      export CC_aarch64_unknown_linux_gnu="${CC_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-gcc}"
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:-aarch64-linux-gnu-gcc}"
    fi
    ;;
esac

cd "$repo_root"
rustup run "$toolchain" cargo build --release --target "$linux_target" --bin mafia
