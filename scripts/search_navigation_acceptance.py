#!/usr/bin/env python3
"""Finite normal-agent Project Search keyboard navigation proof on Linux and Windows.

Compile, exact ignored-test selection, and execution have independent bounds.
The native proof owns its 60 s active / 65 s total deadline and both agents.
Only its complete, strictly validated receipt attests to agent cleanup. Killing
the driver's direct child never substitutes for a missing native receipt.
Requires Python 3.12 for nonblocking anonymous pipes on Windows.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tomllib

# Share the already adversarially tested bounded subprocess/artifact reader;
# keep this feature's receipt, selection, time limits and execution independent.
from copy_draft_acceptance import (
    AcceptanceError, MAX_COMPILE_BYTES, MAX_LIST_BYTES, MAX_RUNTIME_BYTES,
    compiled_test, decode_output, require, strict_json,
    test_compile_command, typed_equal,
)

from search_navigation_diagnostics import write_failure_artifact

from report_triage_diagnostics import (
    StageFailure, bounded_process, run_stage,
)

TEST_NAME = "search_navigation_process_tests::normal_agent_search_keyboard_navigation_is_local_and_source_bound"
PREFIX = "search_navigation_acceptance "
COMPILE_TIMEOUT = 120
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 90
SOURCE_VERSION = "0.63.2"
MAX_AGENT_BYTES = 256 * 1024 * 1024
FILE_ROWS = 96
ROWS = FILE_ROWS * 2
QUERY = "needle café 😀"
CASES = (
    "explicit_trust_off_search",
    "exact_path_line_identity_order",
    "arrow_boundary_clamp",
    "home_end_reveal",
    "dirty_enter_zero_read_exact_back",
    "unopened_enter_exact_one_read",
    "ready_batch_capture_rejected",
    "newer_search_rejects_late_reply",
    "stale_session_capture_rejected",
    "fresh_reconnect_navigation",
    "stale_query_capture_rejected",
    "empty_results_inert",
    "dirty_full_selection_native_undo_redo",
    "exact_ledger_disk_and_owner_cleanup",
)


def generated_text(header):
    return (f"{header} α😀e\u0301 café\r\n"
            + ''.join(f"{QUERY} duplicate {index:03} Ω\r\n" for index in range(FILE_ROWS)))


def expected_receipt(platform):
    require(platform in ("linux", "windows"), "unsupported_platform")
    return {
        "schema": 1, "acceptance": "search_navigation", "success": True, "os": platform,
        "source_version": SOURCE_VERSION, "source_agent_version_match": True,
        "cases": list(CASES), "cases_executed": 14,
        "operation_ledger": [
            {"session": 1, "phase": "setup", "op": "Hello"},
            {"session": 1, "phase": "setup", "op": "List", "source": "root"},
            {"session": 1, "phase": "setup", "op": "Read", "source": "draft"},
            {"session": 1, "phase": "setup", "op": "Search", "query": "matches", "limit": 500},
            {"session": 1, "phase": "activation", "op": "Read", "source": "unopened"},
            {"session": 1, "phase": "refresh", "op": "Search", "query": "matches", "limit": 500},
            {"session": 1, "phase": "refresh", "op": "Search", "query": "matches", "limit": 500},
            {"session": 1, "phase": "refresh", "op": "Search", "query": "matches", "limit": 500},
            {"session": 2, "phase": "reconnect", "op": "Hello"},
            {"session": 2, "phase": "reconnect", "op": "List", "source": "root"},
            {"session": 2, "phase": "reconnect", "op": "Search", "query": "matches", "limit": 500},
            {"session": 2, "phase": "empty", "op": "Search", "query": "empty", "limit": 500},
        ],
        "requests": 12, "hello": 2, "list": 2, "read": 2, "search": 6, "write": 0,
        "other_operations": 0, "movement_operations": 0,
        "dirty_activation_reads": 0, "unopened_activation_reads": 1,
        "setup_requests": 4, "activation_requests": 1, "refresh_requests": 3,
        "reconnect_requests": 3, "empty_requests": 1,
        "connections": 2, "agents_reaped": 2, "source_hashes": [
            {"source": "draft", "sha256": hashlib.sha256(generated_text("saved").encode()).hexdigest()},
            {"source": "unopened", "sha256": hashlib.sha256(generated_text("target").encode()).hexdigest()},
        ],
        "result_rows": 192, "fixture_files": 2,
        "trust_off": True, "no_language_session": True, "production_keyboard_frames": True,
        "exact_path_line_identity_order": True, "arrows_clamp_without_wrap": True,
        "home_end_reveal_visible_row": True, "dirty_enter_zero_read_exact_back": True,
        "unopened_enter_exact_one_read": True, "ready_batch_capture_rejected": True,
        "newer_search_rejects_late_reply": True, "stale_session_capture_rejected": True,
        "fresh_reconnect_navigation": True, "stale_query_capture_rejected": True,
        "empty_results_inert": True, "dirty_text_and_baseline_preserved": True,
        "full_selection_affinity_preserved": True, "complete_native_history_preserved": True,
        "native_undo_redo_preserved": True, "source_disk_unchanged": True,
        "exact_operation_ledger_verified": True,
        "shared_watchdog_seconds": 60, "total_budget_seconds": 65,
        "request_admission_seconds": 30, "cleanup_timeout_seconds": 5,
        "owned_children_reaped": True, "watchdog_joined": True, "fixture_removed": True,
        "optional_font_probe_untriggered": True,
    }


def validate_test_listing(data):
    lines = [line for line in decode_output(data, MAX_LIST_BYTES).splitlines() if line]
    require(lines == [TEST_NAME + ": test", "1 test, 0 benchmarks"], "exact_one_test_not_selected")


def validate_execution(data, platform):
    text = decode_output(data, MAX_RUNTIME_BYTES)
    lines = text.splitlines()
    require([line for line in lines if line.startswith("running ")] == ["running 1 test"],
            "exact_one_test_not_executed")
    results = [line for line in lines if line.startswith("test result:")]
    require(len(results) == 1 and re.fullmatch(
        r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; "
        r"[0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s", results[0]),
        "exact_one_test_not_executed")
    require(text.count(PREFIX) == 1, "exact_one_receipt_required")
    marked = [line for line in lines if PREFIX in line]
    require(len(marked) == 1, "exact_one_receipt_required")
    before, value = marked[0].split(PREFIX, 1)
    require(before in ("", "test " + TEST_NAME + " ... "), "invalid_receipt_prefix")
    executions = [line for line in lines if line.startswith("test ")
                  and not line.startswith("test result:")]
    selected_prefix = "test " + TEST_NAME + " ... "
    require(len(executions) == 1 and executions[0].startswith(selected_prefix),
            "selected_test_execution_missing")
    suffix = executions[0][len(selected_prefix):]
    require(suffix == "ok" or (before == selected_prefix and suffix == PREFIX + value),
            "selected_test_execution_missing")
    receipt = strict_json(value)
    require(typed_equal(receipt, expected_receipt(platform)), "fixed_receipt_contract_mismatch")
    return receipt


def validate_source_version(root):
    manifest = root / "Cargo.toml"
    require(manifest.is_file() and not manifest.is_symlink(), "source_manifest_missing")
    with manifest.open("rb") as stream:
        data = stream.read(64 * 1024 + 1)
    require(len(data) <= 64 * 1024, "source_manifest_oversized")
    try:
        value = tomllib.loads(data.decode("utf-8"))
    except (UnicodeError, tomllib.TOMLDecodeError):
        raise AcceptanceError("source_manifest_invalid") from None
    workspace = value.get("workspace")
    require(type(workspace) is dict, "source_version_mismatch")
    package = workspace.get("package")
    require(type(package) is dict and type(package.get("version")) is str
            and package["version"] == SOURCE_VERSION, "source_version_mismatch")


def agent_digest(binary):
    require(binary.is_file() and not binary.is_symlink()
            and binary.resolve(strict=True) == binary, "release_agent_missing")
    size = binary.stat().st_size
    require(0 < size <= MAX_AGENT_BYTES, "release_agent_size_invalid")
    digest = hashlib.sha256()
    remaining = size
    with binary.open("rb") as stream:
        while remaining:
            block = stream.read(min(65536, remaining))
            require(bool(block), "release_agent_changed")
            digest.update(block)
            remaining -= len(block)
        require(stream.read(1) == b"", "release_agent_changed")
    return size, digest.digest()


def run(root):
    root = Path(root).resolve(strict=True)
    platform = {"linux": "linux", "win32": "windows"}.get(sys.platform)
    require(platform is not None, "unsupported_platform")
    validate_source_version(root)
    binary = root / "target/release" / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
    original_agent = agent_digest(binary)
    environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"),
                       CEDAR_SEARCH_NAVIGATION_AGENT_BIN=str(binary))
    executable = run_stage("compile", root, test_compile_command(), environment,
                           COMPILE_TIMEOUT, MAX_COMPILE_BYTES,
                           lambda data: compiled_test(data, root, platform), bounded_process)
    run_stage("selection", root,
              [str(executable), TEST_NAME, "--ignored", "--exact", "--list"], environment,
              SELECTION_TIMEOUT, MAX_LIST_BYTES, validate_test_listing, bounded_process)
    receipt = run_stage("runtime", root,
                        [str(executable), TEST_NAME, "--ignored", "--exact",
                         "--test-threads=1", "--nocapture"], environment,
                        RUNTIME_TIMEOUT, MAX_RUNTIME_BYTES,
                        lambda data: validate_execution(data, platform), bounded_process)
    validate_source_version(root)
    require(agent_digest(binary) == original_agent, "release_agent_changed")
    print(PREFIX + json.dumps(receipt, ensure_ascii=True, sort_keys=True, allow_nan=False))
    print("Project Search keyboard navigation acceptance passed: 14 cases, 12 explicit requests, 0 movement requests, 2 agents reaped.")
    return receipt


def main():
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parent.parent)
    except StageFailure as error:
        artifact = write_failure_artifact(error, os.environ.get("CEDAR_SEARCH_NAVIGATION_DIAGNOSTIC_DIR"))
        print("Project Search keyboard navigation acceptance failed: " + str(error)
              + "; sanitized_diagnostic=" + artifact, file=sys.stderr)
        return 1
    except AcceptanceError as error:
        print("Project Search keyboard navigation acceptance failed: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError):
        print("Project Search keyboard navigation acceptance failed: driver_error", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
