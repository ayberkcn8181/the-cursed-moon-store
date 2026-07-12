#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
echo "== cargo fmt --check =="
cargo fmt --all -- --check
echo "== cargo test --workspace =="
cargo test --workspace
echo "== cargo build -p tcms-app =="
cargo build -p tcms-app
echo "All checks passed."
