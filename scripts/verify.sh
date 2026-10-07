#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo build --workspace --all-features --locked
export CEDAR_AGENT_BIN="$PWD/target/debug/cedar-agent"
cargo test -p cedar-client --test stdio_roundtrip --locked -- --ignored
python3 scripts/protocol_smoke.py "$CEDAR_AGENT_BIN"
python3 scripts/language_bridge_smoke.py "$CEDAR_AGENT_BIN" "$PWD/target/debug/cedar-mock-lsp"
python3 scripts/task_bridge_smoke.py "$CEDAR_AGENT_BIN"
