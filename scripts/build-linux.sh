#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_HOME="$repo_root/.cargo"
export RUSTUP_HOME="$repo_root/.rustup"
export PATH="$CARGO_HOME/bin:$PATH"

if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
  echo "Repo-local Rust missing. Run scripts/bootstrap-linux-rust.sh first." >&2
  exit 1
fi

cd "$repo_root"
cargo build --release
