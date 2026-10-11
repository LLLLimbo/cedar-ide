#!/usr/bin/env python3
"""Finite normal-agent report keyboard triage proof on Linux and Windows.

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

from report_triage_diagnostics import (
    StageFailure, bounded_process, run_stage, write_failure_artifact,
)

TEST_NAME = "report_triage_process_tests::normal_agent_report_keyboard_triage_is_local_and_revision_bound"
PREFIX = "report_triage_acceptance "
COMPILE_TIMEOUT = 120
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 90
SOURCE_VERSION = "0.59.0"
MAX_AGENT_BYTES = 256 * 1024 * 1024
DRAFT_BASE = "saved α😀e\u0301 café\r\nsecond Ω\r\n"
ROWS = 192
CASES = (
    'explicit_trust_off_report_read',
    'filtered_xml_order_duplicate_identity',
    'arrow_boundary_clamp',
    'home_end_reveal',
    'enter_details_back_exact_row',
    'space_details_back_exact_row',
    'name_filter_empty_no_retarget',
    'stale_filter_capture_rejected',
    'ready_read_capture_rejected',
    'newer_load_rejects_late_read',
    'stale_session_capture_rejected',
    'fresh_reconnect_navigation',
    'dirty_full_selection_native_undo_redo',
    'automatic_height_reset_preserves_report_and_draft',
    'exact_ledger_disk_and_owner_cleanup',
)


def generated_report():
    parts = ['<testsuite name="generated café 😀" tests="192" failures="64" errors="0" skipped="0">\n']
    for index in range(ROWS):
        parts.append(f'<testcase name="duplicate café 😀" classname="fixture.Class{index:03}" time="0.25">')
        if index % 3 == 1:
            parts.append(f'<failure message="marker {index:03}" type="AssertionError">trace Ω marker {index:03}</failure>')
        parts.append('</testcase>\n')
    parts.append('</testsuite>\n')
    return ''.join(parts)


def expected_receipt(platform):
    require(platform in ("linux", "windows"), "unsupported_platform")
    return {
        "schema": 1, "acceptance": "report_triage", "success": True, "os": platform,
        "source_version": SOURCE_VERSION, "source_agent_version_match": True,
        "cases": list(CASES), "cases_executed": 15,
        "operation_ledger": [
            {"session": 1, "phase": "setup", "op": "Hello"},
            {"session": 1, "phase": "setup", "op": "List", "source": "root"},
            {"session": 1, "phase": "setup", "op": "Read", "source": "draft"},
            {"session": 1, "phase": "setup", "op": "Read", "source": "report"},
            {"session": 1, "phase": "refresh", "op": "Read", "source": "report"},
            {"session": 1, "phase": "refresh", "op": "Read", "source": "report"},
            {"session": 1, "phase": "refresh", "op": "Read", "source": "report"},
            {"session": 2, "phase": "reconnect", "op": "Hello"},
            {"session": 2, "phase": "reconnect", "op": "List", "source": "root"},
            {"session": 2, "phase": "reconnect", "op": "Read", "source": "report"},
        ],
        "requests": 10, "hello": 2, "list": 2, "read": 6, "write": 0,
        "other_operations": 0, "navigation_operations": 0,
        "setup_requests": 4, "refresh_requests": 3, "reconnect_requests": 3,
        "connections": 2, "agents_reaped": 2, "source_hashes": [
            {"source": "draft", "sha256": hashlib.sha256(DRAFT_BASE.encode()).hexdigest()},
            {"source": "report", "sha256": hashlib.sha256(generated_report().encode()).hexdigest()},
        ],
        "report_rows": 192, "filtered_rows": 64, "first_filtered_index": 1, "last_filtered_index": 190,
        "trust_off": True,
        "no_language_session": True,
        "production_keyboard_frames": True,
        "filtered_original_xml_indices": True,
        "duplicate_names_distinct_identity": True,
        "arrows_clamp_without_wrap": True,
        "home_end_reveal_visible_row": True,
        "enter_space_details_back_exact_row": True,
        "empty_filter_cannot_retarget": True,
        "stale_filter_capture_rejected": True,
        "ready_read_capture_rejected": True,
        "newer_load_rejects_late_read": True,
        "stale_session_capture_rejected": True,
        "fresh_reconnect_navigation": True,
        "dirty_text_and_baseline_preserved": True,
        "full_selection_affinity_preserved": True,
        "complete_native_history_preserved": True,
        "native_undo_redo_preserved": True,
        "automatic_height_reset_preserved_state": True,
        "source_disk_unchanged": True,
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
                       CEDAR_REPORT_TRIAGE_AGENT_BIN=str(binary))
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
    print("Report keyboard triage acceptance passed: 15 cases, 10 explicit requests, 0 navigation requests, 2 agents reaped.")
    return receipt


def main():
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parent.parent)
    except StageFailure as error:
        artifact = write_failure_artifact(error, os.environ.get("CEDAR_REPORT_TRIAGE_DIAGNOSTIC_DIR"))
        print("Report keyboard triage acceptance failed: " + str(error)
              + "; sanitized_diagnostic=" + artifact, file=sys.stderr)
        return 1
    except AcceptanceError as error:
        print("Report keyboard triage acceptance failed: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError):
        print("Report keyboard triage acceptance failed: driver_error", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
