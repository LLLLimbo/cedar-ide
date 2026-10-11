#!/usr/bin/env python3
"""Finite normal-agent Ctrl+G line:column proof on Linux and Windows.

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

# Share the already adversarially tested bounded subprocess/artifact reader;
# keep this feature's receipt, selection, time limits and execution independent.
from copy_draft_acceptance import (
    AcceptanceError, MAX_COMPILE_BYTES, MAX_LIST_BYTES, MAX_RUNTIME_BYTES,
    bounded_process, compiled_test, decode_output, require, strict_json,
    test_compile_command, typed_equal,
)

TEST_NAME = "column_navigation_process_tests::normal_agent_line_column_navigation_is_local_and_preserves_history"
PREFIX = "column_navigation_acceptance "
COMPILE_TIMEOUT = 120
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 90
FIRST = "coordinates 😀/CRLF café.txt"
SECOND = "coordinates 😀/LF Ω.txt"
FIRST_BASE = "\tα😀e\u0301 café\r\nβ\tΩ\r\n\r\nlast\r\n"
SECOND_BASE = "\tα😀e\u0301 café\nβ\tΩ\n\nlast\n"
CASES = (
    "unicode_crlf_coordinates", "unicode_lf_coordinates", "plain_line_compatibility",
    "invalid_coordinates_preserve_state", "cancel_preserves_full_selection",
    "same_location_preserves_history", "dirty_undo_redo",
    "stale_edit_before_release", "stale_edit_after_release",
    "stale_tab_before_release", "stale_navigation_after_release",
    "oversized_input_refused", "stale_reconnect", "history_after_reconnect",
)


def expected_receipt(platform):
    require(platform in ("linux", "windows"), "unsupported_platform")
    return {
        "schema": 1, "acceptance": "column_navigation", "success": True, "os": platform,
        "cases": list(CASES), "cases_executed": 14,
        "operation_ledger": [
            {"session": 1, "op": "Hello"}, {"session": 1, "op": "List", "path": ""},
            {"session": 1, "op": "Read", "path": FIRST},
            {"session": 1, "op": "Read", "path": SECOND},
            {"session": 2, "op": "Hello"}, {"session": 2, "op": "List", "path": ""},
        ],
        "requests": 6, "hello": 2, "list": 2, "read": 2, "write": 0,
        "other_operations": 0, "measured_navigation_operations": 0,
        "connections": 2, "agents_reaped": 2, "source_hashes": [
            {"path": FIRST, "sha256": hashlib.sha256(FIRST_BASE.encode()).hexdigest()},
            {"path": SECOND, "sha256": hashlib.sha256(SECOND_BASE.encode()).hexdigest()},
        ],
        "trust_off": True, "production_keyboard_frames": True,
        "unicode_scalar_columns": True, "lf_crlf_logical_end": True,
        "trailing_empty_line": True, "plain_line_compatible": True,
        "invalid_and_oversized_preserved": True, "dirty_baselines_preserved": True,
        "full_selection_and_affinity_preserved": True, "back_forward_preserved": True,
        "undo_redo_preserved": True, "stale_delayed_coordinates_rejected": True,
        "reconnect_rejected_old_coordinate": True, "exact_operation_ledger_verified": True,
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
    receipt = strict_json(value)
    require(typed_equal(receipt, expected_receipt(platform)), "fixed_receipt_contract_mismatch")
    return receipt


def run(root):
    root = Path(root).resolve(strict=True)
    platform = {"linux": "linux", "win32": "windows"}.get(sys.platform)
    require(platform is not None, "unsupported_platform")
    binary = root / "target/release" / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
    require(binary.is_file() and not binary.is_symlink()
            and binary.resolve(strict=True) == binary, "release_agent_missing")
    environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"),
                       CEDAR_COLUMN_NAVIGATION_AGENT_BIN=str(binary))
    compiled = bounded_process(test_compile_command(), root, environment,
                               COMPILE_TIMEOUT, MAX_COMPILE_BYTES)
    executable = compiled_test(compiled, root, platform)
    selected = bounded_process([str(executable), TEST_NAME, "--ignored", "--exact", "--list"],
                               root, environment, SELECTION_TIMEOUT, MAX_LIST_BYTES)
    validate_test_listing(selected)
    executed = bounded_process([str(executable), TEST_NAME, "--ignored", "--exact",
                                "--test-threads=1", "--nocapture"], root, environment,
                               RUNTIME_TIMEOUT, MAX_RUNTIME_BYTES)
    receipt = validate_execution(executed, platform)
    print(PREFIX + json.dumps(receipt, ensure_ascii=True, sort_keys=True, allow_nan=False))
    print("Column navigation acceptance passed: 14 cases, 6 setup requests, 2 agents reaped.")
    return receipt


def main():
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parent.parent)
    except AcceptanceError as error:
        print("Column navigation acceptance failed: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError):
        print("Column navigation acceptance failed: driver_error", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
