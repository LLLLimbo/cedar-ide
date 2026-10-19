#!/usr/bin/env python3
"""Run one exact Match Case journey against the ordinary release agent.

The aggregate all-feature graph is reused without building another agent.
Compilation has a 120-second outer bound; the native proof owns its
340-second active / 30-second cleanup / 390-second watchdog budgets.
The driver's 420-second runtime bound observes termination, not extra
success time. Direct-child termination never establishes native cleanup.
Version equality and unchanged release bytes do not attest source provenance.
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

TEST_NAME = "case_search_process_tests::normal_agent_case_search_is_explicit_local_and_source_bound"
PREFIX = "case_search_acceptance "
PROGRESS_PREFIX = "case_search_progress "
COMPILE_TIMEOUT = 120
RUNTIME_TIMEOUT = 420
SOURCE_VERSION = "0.79.0"
QUERY = "K [x].* café 😀"
CASES = [
    "trust_off_normal_agent",
    "checked_legacy_root",
    "checked_filtered_route",
    "shared_native_case_controls_local",
    "unchecked_directory_echo",
    "local_grouping_and_movement",
    "dirty_preview_and_return",
    "unopened_preview",
    "toggle_back_retires_search_open",
    "fresh_root_cached_open_exact_back_undo",
    "nested_reply_retired_before_delivery",
    "root_basename_mask",
    "complete_no_match",
    "local_edit_retirement",
    "retained_shell_and_provider_state",
    "exact_ledger_disk_and_owner_cleanup",
]

# Fixed test inputs, independent of subprocess receipts and producer output.
FIXTURES = (
    ("origin", "src Ω/a origin.rs", "saved", "K"),
    ("target", "src Ω/b target.rs", "target", "K"),
    ("nested", "src Ω/nested/c note.md", "nested", "k"),
    ("root", "root.rs", "root", "K"),
    ("outside", "outside/d untouched.txt", "outside", "k"),
    ("masked", "src Ω/z masked.RS", "masked", "K"),
)


def fixture_bytes(source, header, variant):
    return (f"{header} Ω😀\r\n{variant} [x].* café 😀 {source}\r\n"
            "K xZZ café 😀 decoy\r\n"
            + ("last\r\n" if source == "origin" else "")).encode("utf-8")


def expected_receipt(platform="linux"):
    """The fixed consumer contract; never learn expectations from a receipt."""
    require(platform in ("linux", "windows"), "unsupported_platform")

    def request(scope, masks, case=False):
        value = {"query": QUERY, "scope": scope, "masks": masks, "limit": 500}
        if case:
            value["case_sensitive"] = False
        return value

    disk = {source: fixture_bytes(source, header, variant)
            for source, _path, header, variant in FIXTURES}
    matches = {source: {"path": path, "line": 2,
                        "text": f"{variant} [x].* café 😀 {source}"}
               for source, path, _header, variant in FIXTURES}

    def observation(phase, route, sources, accepted_rows, examined_entries):
        rows = sorted((matches[source] for source in sources), key=lambda row: row["path"])
        return {
            "phase": phase, "route": route,
            "returned_rows": len(rows), "accepted_rows": accepted_rows,
            "matches": rows, "examined_entries": examined_entries,
            "read_bytes": None if route == "Legacy" else sum(len(disk[source]) for source in sources),
        }

    return {
        "schema": 1, "acceptance": "case_search", "success": True, "os": platform,
        "source_version": SOURCE_VERSION, "source_agent_version_match": True,
        "cases": list(CASES), "cases_executed": 16,
        "operation_ledger": [
            {"phase": "setup", "op": "Hello"},
            {"phase": "setup", "op": "List", "source": "root_directory"},
            {"phase": "setup", "op": "Read", "source": "origin"},
            {"phase": "legacy", "op": "Search", "query": QUERY, "limit": 500},
            {"phase": "checked_directory", "op": "SearchIn", "request": request("src Ω", ["*.rs"])},
            {"phase": "directory", "op": "SearchCase", "request": request("src Ω", ["*.rs"], True)},
            {"phase": "preview", "op": "Read", "source": "target"},
            {"phase": "retired_open", "op": "Read", "source": "target"},
            {"phase": "root", "op": "SearchCase", "request": request("", [], True)},
            {"phase": "nested", "op": "SearchCase", "request": request("src Ω/nested", ["*.rs", "*.md"], True)},
            {"phase": "root_mask", "op": "SearchCase", "request": request("", ["*.rs"], True)},
            {"phase": "empty", "op": "SearchCase", "request": request("src Ω", ["*.zz"], True)},
        ],
        "source_hashes": [{"source": source, "sha256": hashlib.sha256(disk[source]).hexdigest()}
                          for source, _path, _header, _variant in FIXTURES],
        "search_observations": [
            observation("legacy", "Legacy", ("origin", "root"), 2, None),
            # Eligible bytes include the nonmatching Kelvin-sign target.
            {**observation("checked_directory", "ScopedV1", ("origin",), 1, 5),
             "read_bytes": len(disk["origin"]) + len(disk["target"])},
            observation("directory", "CaseV1", ("origin", "target"), 2, 5),
            observation("root", "CaseV1", tuple(disk), 6, 9),
            observation("nested", "CaseV1", ("nested",), 0, 1),
            observation("root_mask", "CaseV1", ("origin", "target", "root"), 3, 9),
            observation("empty", "CaseV1", (), 0, 5),
        ],
        "requests": 12, "hello": 1, "list": 1, "read": 3,
        "search": 1, "search_in": 1, "search_case": 5, "write": 0, "other_operations": 0,
        "connection_attempts": 1, "connections": 1, "agents_reaped": 1, "fixture_files": 6,
        "movement_operations": 0, "filter_edit_operations": 0, "toggle_operations": 0,
        "dirty_preview_reads": 0, "unopened_preview_reads": 1,
        "retired_unopened_open_reads": 1, "fresh_cached_open_reads": 0,
        "shell_transition_operations": 0,
        "trust_off": True, "no_language_session": True, "no_recovery_actor": True,
        "production_native_frames": True, "recording_worker_client_bridge": True,
        "default_match_case_checked": True, "shared_match_case_controls": True,
        "native_pointer_space_enter_toggle": True, "exact_scoped_and_case_echo_validated": True,
        "literal_metacharacters_preserved": True, "original_unicode_snippets_preserved": True,
        "mask_case_sensitive": True, "complete_no_match": True,
        "toggle_back_does_not_revive": True, "retired_open_cached_inactive": True,
        "retired_open_no_activation_or_history": True, "retired_case_app_job_drained": True,
        "retired_case_no_active_rows_preview_or_error": True,
        "dirty_text_and_baseline_preserved": True, "edit_version_preserved_between_edits": True,
        "full_selection_affinity_preserved": True, "complete_native_history_preserved": True,
        "native_undo_redo_preserved": True, "source_disk_unchanged": True,
        "exact_operation_ledger_verified": True, "optional_font_probe_untriggered": True,
        "shell_project_focus_retains_find": True, "shell_find_reopen_query_fallback": True,
        "shell_hide_exact_editor_return": True, "shell_provider_retains_match_case": True,
        "shell_state_preserved": True,
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
                       CEDAR_CASE_SEARCH_AGENT_BIN=str(agent))
    artifacts = checked_stage("compile", compile_command(), root, environment,
                              COMPILE_TIMEOUT, MAX_COMPILE_BYTES,
                              lambda data: select_artifacts(data, root, platform), runner)
    command = [str(artifacts["cedar_app"]), TEST_NAME, "--ignored", "--exact",
               "--test-threads=1", "--nocapture"]
    receipt = checked_stage("runtime", command, root, environment,
                            RUNTIME_TIMEOUT, MAX_RUNTIME_BYTES,
                            lambda data: validate_execution(data, platform), runner)
    require(agent_digest(agent) == original_agent, "release_agent_changed")
    print("Match Case acceptance passed: 12 explicit operations, no local navigation or toggle operations, one agent reaped.")
    return receipt


def main():
    # Preserve captured UTF-8 and non-BMP text even on redirected cp1252 streams.
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="strict", newline="")
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parents[1])
    except (AcceptanceError, OSError) as error:
        print("Match Case acceptance failed: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
