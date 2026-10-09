#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
python3 scripts/test_public_export.py
python3 scripts/test_windows_bundle.py
python3 scripts/test_java_crash_collection.py
python3 scripts/test_prepare_maven_cache.py
python3 scripts/test_maven_acceptance.py
python3 scripts/test_process_tree_baseline.py
python3 scripts/test_gc_control_collection.py
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo build --workspace --all-features --locked
target_dir="$(cargo metadata --no-deps --format-version 1 --offline --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
export CEDAR_AGENT_BIN="$target_dir/debug/cedar-agent"
export CEDAR_INTERRUPTED_SAVE_AGENT_BIN="$target_dir/debug/cedar-agent-interrupted-save-validation"
export CEDAR_CONNECTION_CANCEL_PEER_BIN="$target_dir/debug/cedar-client-transport-peer"
cargo test -p cedar-app --lib connection_cancel_tests --locked -- --ignored --test-threads=1
cargo test -p cedar-app --lib interrupted_save_process_tests --locked -- --ignored --test-threads=1
cargo test -p cedar-client --test stdio_roundtrip --locked -- --ignored
python3 scripts/protocol_smoke.py "$CEDAR_AGENT_BIN"
python3 scripts/capability_smoke.py "$CEDAR_AGENT_BIN"
python3 scripts/language_bridge_smoke.py "$CEDAR_AGENT_BIN" "$target_dir/debug/cedar-mock-lsp"
python3 scripts/task_bridge_smoke.py "$CEDAR_AGENT_BIN"
cargo build --manifest-path examples/task-profiles-demo/Cargo.toml --offline --locked
fixture_target="$(cargo metadata --manifest-path examples/task-profiles-demo/Cargo.toml --no-deps --format-version 1 --offline --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
python3 scripts/task_profiles_smoke.py "$CEDAR_AGENT_BIN" "$fixture_target/debug/cedar-task-profile-demo"
python3 scripts/git_views_smoke.py --agent "$CEDAR_AGENT_BIN" --git "$(command -v git)"
