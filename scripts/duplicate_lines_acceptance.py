#!/usr/bin/env python3
"""Finite normal-agent duplicate-lines proof on Linux and Windows.

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
    bounded_process, compiled_test, decode_output, require, strict_json,
    test_compile_command, typed_equal,
)

TEST_NAME = "duplicate_lines_process_tests::normal_agent_duplicate_lines_is_local_and_preserves_history"
PREFIX = "duplicate_lines_acceptance "
COMPILE_TIMEOUT = 120
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 90
SOURCE_VERSION = "0.58.2"
MAX_AGENT_BYTES = 256 * 1024 * 1024
FIRST = "duplicate 😀/CRLF café.txt"
SECOND = "duplicate 😀/LF Ω.txt"
FIRST_BASE = "\tα😀e\u0301 café\r\nβ\tΩ\r\n\r\nlast\r\n"
SECOND_BASE = "\tα😀e\u0301 café\nβ\tΩ\n\nlast\n"
CASES = (
    'unicode_crlf_collapsed',
    'unicode_lf_collapsed',
    'unicode_crlf_forward_block',
    'unicode_crlf_reversed_block',
    'unicode_lf_forward_block',
    'unicode_lf_reversed_block',
    'upper_endpoint_line_start_excluded',
    'mixed_line_endings_exact',
    'unterminated_nearest_separator',
    'empty_and_trailing_blank',
    'strict_modifiers_and_repeat_refused',
    'mixed_edit_frames_native_only',
    'focus_and_modal_guards',
    'stale_identity_and_selection_guards',
    'byte_and_line_bounds_refused',
    'dirty_baselines_and_other_tab_preserved',
    'atomic_full_selection_undo_redo',
    'stale_reconnect_capture_rejected',
    'history_after_reconnect',
    'duplication_after_reconnect',
)


def expected_receipt(platform):
    require(platform in ("linux", "windows"), "unsupported_platform")
    return {
        "schema": 1, "acceptance": "duplicate_lines", "success": True, "os": platform,
        "source_version": SOURCE_VERSION, "source_agent_version_match": True,
        "cases": list(CASES), "cases_executed": 20,
        "operation_ledger": [
            {"session": 1, "op": "Hello"}, {"session": 1, "op": "List", "path": ""},
            {"session": 1, "op": "Read", "path": FIRST},
            {"session": 1, "op": "Read", "path": SECOND},
            {"session": 2, "op": "Hello"}, {"session": 2, "op": "List", "path": ""},
        ],
        "requests": 6, "hello": 2, "list": 2, "read": 2, "write": 0,
        "other_operations": 0, "measured_duplicate_operations": 0,
        "connections": 2, "agents_reaped": 2, "source_hashes": [
            {"path": FIRST, "sha256": hashlib.sha256(FIRST_BASE.encode()).hexdigest()},
            {"path": SECOND, "sha256": hashlib.sha256(SECOND_BASE.encode()).hexdigest()},
        ],
        'trust_off': True,
        'no_language_session': True,
        'production_keyboard_frames': True,
        'unicode_scalar_endpoints': True,
        'lf_crlf_mixed_bytes_preserved': True,
        'forward_reversed_selection': True,
        'upper_line_start_excluded': True,
        'unterminated_nearest_separator': True,
        'empty_trailing_blank_supported': True,
        'strict_modifiers_repeat_refused': True,
        'mixed_input_not_duplicated': True,
        'focus_modal_stale_capture_guarded': True,
        'byte_and_line_bounds_refused': True,
        'dirty_baselines_and_save_state_preserved': True,
        'unrelated_tab_preserved': True,
        'atomic_full_selection_undo_redo': True,
        'complete_native_history_preserved': True,
        'reconnect_rejected_old_capture': True,
        'reconnect_undo_redo_preserved': True,
        'exact_operation_ledger_verified': True,
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
                       CEDAR_DUPLICATE_LINES_AGENT_BIN=str(binary))
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
    validate_source_version(root)
    require(agent_digest(binary) == original_agent, "release_agent_changed")
    print(PREFIX + json.dumps(receipt, ensure_ascii=True, sort_keys=True, allow_nan=False))
    print("Duplicate lines acceptance passed: 20 cases, 6 setup requests, 2 agents reaped.")
    return receipt


def main():
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parent.parent)
    except AcceptanceError as error:
        print("Duplicate lines acceptance failed: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError):
        print("Duplicate lines acceptance failed: driver_error", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
