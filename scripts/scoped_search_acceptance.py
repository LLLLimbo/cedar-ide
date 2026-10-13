#!/usr/bin/env python3
"""Run one exact scoped-search journey against the ordinary release agent.

The existing aggregate feature graph is reused. Compilation has a 120-second
outer bound; the proof has 340 active / 30 cleanup / 390 watchdog seconds.
The driver's 420-second bound observes termination, not extra success time.
Direct-child termination never establishes native agent cleanup.
"""
import hashlib
import os
from pathlib import Path
import re
import sys

from browse_acceptance import compile_command, select_artifacts, checked_stage
from copy_draft_acceptance import (
    AcceptanceError, MAX_COMPILE_BYTES, MAX_RUNTIME_BYTES,
    decode_output, require, strict_json, typed_equal,
)
from report_triage_diagnostics import bounded_process
from search_navigation_acceptance import agent_digest

TEST_NAME = "scoped_search_process_tests::normal_agent_scoped_search_is_explicit_local_and_source_bound"
PREFIX = "scoped_search_acceptance "
COMPILE_TIMEOUT = 120
RUNTIME_TIMEOUT = 420
SOURCE_VERSION = "0.63.1"
QUERY = "needle café 😀"
CASES = [
    "trust_off_normal_agent", "legacy_search_compatibility", "explicit_directory_and_mask",
    "local_grouping_and_movement", "dirty_preview_and_return", "unopened_preview_then_open",
    "exact_back_and_native_undo", "nested_multi_mask", "root_basename_mask", "complete_no_match",
    "local_filter_edit_retirement", "exact_ledger_disk_and_owner_cleanup",
]


def expected_receipt(platform="linux"):
    require(platform in ("linux", "windows"), "unsupported_platform")
    def request(scope, masks):
        return {"query": QUERY, "scope": scope, "masks": masks, "limit": 500}
    hashes = []
    for source, header in (("origin", "saved"), ("target", "target"),
                           ("nested", "nested"), ("root", "root"), ("outside", "outside")):
        contents = f"{header} Ω😀\r\n{QUERY} {source}\r\n" + ("last\r\n" if source == "origin" else "")
        hashes.append({"source": source, "sha256": hashlib.sha256(contents.encode("utf-8")).hexdigest()})
    return {
        "schema": 1, "acceptance": "scoped_search", "success": True, "os": platform,
        "source_version": SOURCE_VERSION, "source_agent_version_match": True,
        "cases": CASES, "cases_executed": 12,
        "operation_ledger": [
            {"phase": "setup", "op": "Hello"},
            {"phase": "setup", "op": "List", "source": "root_directory"},
            {"phase": "setup", "op": "Read", "source": "origin"},
            {"phase": "legacy", "op": "Search", "query": QUERY, "limit": 500},
            {"phase": "directory", "op": "SearchIn", "request": request("src Ω", ["*.rs"])},
            {"phase": "activation", "op": "Read", "source": "target"},
            {"phase": "activation", "op": "Read", "source": "target"},
            {"phase": "nested", "op": "SearchIn", "request": request("src Ω/nested", ["*.rs", "*.md"])},
            {"phase": "root_mask", "op": "SearchIn", "request": request("", ["*.rs"])},
            {"phase": "empty", "op": "SearchIn", "request": request("src Ω", ["*.zz"])},
        ],
        "source_hashes": hashes, "requests": 10, "hello": 1, "list": 1, "read": 3,
        "search": 1, "search_in": 4, "write": 0, "other_operations": 0,
        "connections": 1, "agents_reaped": 1, "fixture_files": 5,
        "movement_operations": 0, "filter_edit_operations": 0, "dirty_preview_reads": 0,
        "unopened_preview_reads": 1, "unopened_open_reads": 1,
        "legacy_rows": 5, "directory_rows": 2, "nested_rows": 1, "root_mask_rows": 3, "empty_rows": 0,
        "trust_off": True, "no_language_session": True, "no_recovery_actor": True,
        "production_native_frames": True, "shared_filter_controls": True,
        "exact_scoped_echo_validated": True, "complete_no_match": True,
        "dirty_text_and_baseline_preserved": True, "full_selection_affinity_preserved": True,
        "complete_native_history_preserved": True, "native_undo_redo_preserved": True,
        "source_disk_unchanged": True, "exact_operation_ledger_verified": True,
        "optional_font_probe_untriggered": True,
        "shell_project_focus_retains_find": True, "shell_find_reopen_query_fallback": True,
        "shell_hide_exact_editor_return": True, "shell_state_preserved": True,
        "shell_transition_operations": 0,
        "shared_active_seconds": 340, "request_admission_seconds": 30,
        "cleanup_timeout_seconds": 30, "proof_watchdog_seconds": 390,
        "owned_children_reaped": True, "watchdog_joined": True, "fixture_removed": True,
    }


def validate_execution(data, platform="linux"):
    text = decode_output(data, MAX_RUNTIME_BYTES)
    lines = text.splitlines()
    require([line for line in lines if line.startswith("running ")] == ["running 1 test"],
            "exact_one_test_not_executed")
    results = [line for line in lines if line.startswith("test result:")]
    require(len(results) == 1 and re.fullmatch(
        r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; "
        r"[0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s", results[0]),
        "exact_one_test_not_executed")
    executions = [line for line in lines if line.startswith("test ")
                  and not line.startswith("test result:")]
    selected_prefix = "test " + TEST_NAME + " ... "
    require(len(executions) == 1 and executions[0].startswith(selected_prefix),
            "selected_test_execution_missing")
    require(text.count(PREFIX) == 1, "exact_one_receipt_required")
    marked = next(line for line in lines if PREFIX in line)
    before, value = marked.split(PREFIX, 1)
    require(before in ("", selected_prefix), "invalid_receipt_prefix")
    suffix = executions[0][len(selected_prefix):]
    require(suffix == "ok" or (before == selected_prefix and suffix == PREFIX + value),
            "selected_test_execution_missing")
    receipt = strict_json(value)
    require(typed_equal(receipt, expected_receipt(platform)), "fixed_receipt_contract_mismatch")
    return receipt


def run(root, runner=bounded_process):
    root = Path(root).resolve(strict=True)
    platform = {"linux": "linux", "win32": "windows"}.get(sys.platform)
    require(platform is not None, "unsupported_platform")
    agent = root / "target/release" / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
    original_agent = agent_digest(agent)
    environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"),
                       CEDAR_SCOPED_SEARCH_AGENT_BIN=str(agent))
    artifacts = checked_stage("compile", compile_command(), root, environment,
                              COMPILE_TIMEOUT, MAX_COMPILE_BYTES,
                              lambda data: select_artifacts(data, root, platform), runner)
    command = [str(artifacts["cedar_app"]), TEST_NAME, "--ignored", "--exact",
               "--test-threads=1", "--nocapture"]
    receipt = checked_stage("runtime", command, root, environment,
                            RUNTIME_TIMEOUT, MAX_RUNTIME_BYTES,
                            lambda data: validate_execution(data, platform), runner)
    require(agent_digest(agent) == original_agent, "release_agent_changed")
    print("Scoped Search acceptance passed: 10 explicit operations, no navigation operations, one agent reaped.")
    return receipt


def main():
    # The native receipt contains literal Unicode. Windows redirected streams
    # may otherwise use cp1252 even though the captured child output is UTF-8.
    # Keep the original output intact; imported run() leaves stream policy to
    # its caller, including the in-memory streams used by the driver tests.
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="strict", newline="")
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parents[1])
    except (AcceptanceError, OSError) as error:
        print("Scoped Search acceptance failed: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
