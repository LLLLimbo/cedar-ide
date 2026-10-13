#!/usr/bin/env python3
"""One Linux normal-Client popup proof using the existing compiled mock LSP.

The retained language_bridge_smoke proof remains unchanged. This companion uses
its existing normal fixture through the ordinary Rust Client and actual app
frames. Compile120 + selection15 + runtime420 + three child-reap5 bounds fit the
ten-minute CI step; native clocks remain active330 / cleanup64 / total394.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile

from copy_draft_acceptance import (
    AcceptanceError, MAX_COMPILE_BYTES, MAX_LIST_BYTES, MAX_RUNTIME_BYTES,
    bounded_process, compiled_test, decode_output, require, strict_json,
    test_compile_command, typed_equal,
)

TEST_NAME = ("language_ui::file_structure_process_tests::"
             "normal_agent_file_structure_reuses_snapshot_and_retires_cancelled_reply")
PREFIX = "file_structure_acceptance "
FAILURE_PREFIX = "file_structure_failure "
COMPILE_TIMEOUT = 120
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 420
SOURCE = "class Hello { /* café 😀 */ int count; }\n"
PATH = "Hello.java"
LSP_METHODS = ["initialize", "initialized", "textDocument/didOpen",
               "textDocument/documentSymbol", "textDocument/didChange",
               "textDocument/documentSymbol", "textDocument/documentSymbol",
               "textDocument/didClose", "shutdown", "exit"]
NATIVE_STAGES = ("setup", "connect", "read", "start", "initial_outline", "cache_navigation",
                 "dirty_outline", "cancelled_reply", "dirty_navigation", "cleanup", "audit", "finish")
DRIVER_STAGES = ("setup", "compile", "selection", "runtime", "validation")
MAX_FAILURE_BYTES = 4096


def expected_receipt():
    return {
        "schema": 1, "acceptance": "file_structure", "success": True, "os": "linux",
        "route": "normal_client_normal_agent_mock_lsp", "operation_ledger": [
            {"op": "Hello"}, {"op": "List", "path": ""}, {"op": "Read", "path": PATH},
            {"op": "LanguageStart", "fixture": "cedar-mock-lsp", "mode": "normal"},
            {"op": "LanguageOpen", "path": PATH, "version": 1},
            {"op": "LanguageDocumentSymbols", "path": PATH},
            {"op": "LanguageChange", "path": PATH, "version": 2},
            {"op": "LanguageDocumentSymbols", "path": PATH},
            {"op": "LanguageDocumentSymbols", "path": PATH},
            {"op": "LanguageClose", "path": PATH}, {"op": "LanguageStop"},
        ],
        "requests": 11, "hello": 1, "list": 1, "read": 1, "write": 0, "run": 0,
        "language_start": 1, "language_open": 1, "language_change": 1,
        "language_document_symbols": 3, "language_close": 1, "language_stop": 1,
        "dynamic_operations": 0, "measured_local_operations": 0, "lsp_methods": LSP_METHODS,
        "source_sha256": hashlib.sha256(SOURCE.encode()).hexdigest(),
        "exact_sync_snapshots": True, "cache_filter_zero_operations": True,
        "cancelled_response_retired": True, "popup_response_kept_tool": True,
        "local_selection_and_back": True, "dirty_undo_preserved": True,
        "source_bytes_unchanged": True, "production_keyboard_frames": True,
        "explicit_fixture_start_only": True, "automatic_sync_disabled": True,
        "agent_reaped": True, "generic_stop_cleanup_verified": True,
        "watchdog_joined": True, "fixture_removed": True,
        "active_seconds": 330, "cleanup_reserve_seconds": 64, "total_seconds": 394,
        "request_seconds": 30, "start_seconds": 75, "client_reap_seconds": 4,
        "native_jdt": "unmeasured", "windows_provider": "unmeasured", "native_gui": "unmeasured",
    }


def validate_test_listing(data):
    require([line for line in decode_output(data, MAX_LIST_BYTES).splitlines() if line]
            == [TEST_NAME + ": test", "1 test, 0 benchmarks"], "exact_one_test_not_selected")


def validate_execution(data):
    text = decode_output(data, MAX_RUNTIME_BYTES)
    require(FAILURE_PREFIX not in text, "native_failure_with_success")
    lines = text.splitlines()
    require([line for line in lines if line.startswith("running ")] == ["running 1 test"],
            "exact_one_test_not_executed")
    results = [line for line in lines if line.startswith("test result:")]
    require(len(results) == 1 and re.fullmatch(
        r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; "
        r"[0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s", results[0]),
        "exact_one_test_not_executed")
    marked = [line for line in lines if PREFIX in line]
    require(text.count(PREFIX) == 1 and len(marked) == 1, "exact_one_receipt_required")
    before, value = marked[0].split(PREFIX, 1)
    require(before in ("", "test " + TEST_NAME + " ... "), "invalid_receipt_prefix")
    receipt = strict_json(value)
    require(typed_equal(receipt, expected_receipt()), "fixed_receipt_contract_mismatch")
    return receipt


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def validate_failure(receipt):
    require(type(receipt) is dict, "invalid_failure_receipt")
    common = {"schema": 1, "acceptance": "file_structure", "success": False, "os": "linux"}
    require(all(key in receipt and typed_equal(receipt[key], value)
                for key, value in common.items()), "invalid_failure_receipt")
    if receipt.get("kind") == "native":
        expected = dict(common, kind="native", category="native_check_failed",
                        stage=receipt.get("stage"), operations_recorded=receipt.get("operations_recorded"),
                        agent_owned_before=receipt.get("agent_owned_before"),
                        server_owned_before=receipt.get("server_owned_before"),
                        document_open_before=receipt.get("document_open_before"),
                        agent_reaped_after=receipt.get("agent_reaped_after"),
                        server_cleanup_after=receipt.get("server_cleanup_after"),
                        fixture_retained=receipt.get("fixture_retained"))
        require(receipt.get("stage") in NATIVE_STAGES
                and type(receipt.get("operations_recorded")) is int
                and 0 <= receipt["operations_recorded"] <= 11
                and all(type(receipt.get(key)) is bool for key in (
                    "agent_owned_before", "server_owned_before", "document_open_before",
                    "agent_reaped_after", "fixture_retained"))
                and receipt.get("server_cleanup_after") in ("not_started", "verified", "unverified"),
                "invalid_failure_receipt")
    else:
        expected = dict(common, kind="driver", category=receipt.get("category"),
                        stage=receipt.get("stage"), native_cleanup="unmeasured")
        require(receipt.get("stage") in DRIVER_STAGES and receipt.get("category") in (
            "driver_check_failed", "native_receipt_missing", "native_receipt_invalid"),
            "invalid_failure_receipt")
    require(typed_equal(receipt, expected), "invalid_failure_receipt")
    return receipt


def report_failure(evidence, stage):
    category = "driver_check_failed"
    receipt = None
    native = evidence / "native-failure.json" if evidence else None
    if stage in ("runtime", "validation"):
        category = "native_receipt_missing"
        if native and native.is_file():
            try:
                with native.open("rb") as stream:
                    data = stream.read(MAX_FAILURE_BYTES + 1)
                require(len(data) <= MAX_FAILURE_BYTES, "failure_receipt_too_large")
                receipt = validate_failure(strict_json(data))
                require(receipt["kind"] == "native", "invalid_failure_receipt")
            except (OSError, ValueError):
                category, receipt = "native_receipt_invalid", None
    if receipt is None:
        receipt = validate_failure({"schema": 1, "acceptance": "file_structure", "success": False,
                                    "os": "linux", "kind": "driver", "stage": stage,
                                    "category": category, "native_cleanup": "unmeasured"})
    encoded = json.dumps(receipt, ensure_ascii=True, sort_keys=True, allow_nan=False)
    if evidence:
        try:
            with (evidence / "driver-failure.json").open("x", encoding="ascii") as stream:
                stream.write(encoded + "\n")
        except OSError:
            pass  # The same bounded receipt still survives in CI stdout.
    print(FAILURE_PREFIX + encoded)
    return receipt


def run(root):
    stage, evidence = "setup", None
    try:
        require(sys.platform == "linux", "unsupported_platform")
        root = Path(root).resolve(strict=True)
        environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
        selected = []
        for variable, filename in (("CEDAR_AGENT_BIN", "cedar-agent"),
                                   ("CEDAR_MOCK_LSP_BIN", "cedar-mock-lsp")):
            binary = root / "target/release" / filename
            require(binary.is_file() and not binary.is_symlink()
                    and binary.resolve(strict=True) == binary, "explicit_release_binary_missing")
            environment[variable] = str(binary)
            selected.append(binary)
        evidence = Path(tempfile.mkdtemp(prefix="cedar-file-structure-evidence-"))
        environment["CEDAR_FILE_STRUCTURE_FAILURE_FILE"] = str(evidence / "native-failure.json")
        stage = "compile"
        compiled = bounded_process(test_compile_command(), root, environment,
                                   COMPILE_TIMEOUT, MAX_COMPILE_BYTES)
        executable = compiled_test(compiled, root, "linux")
        selected.append(executable)
        before = [digest(path) for path in selected]
        stage = "selection"
        listing = bounded_process([str(executable), TEST_NAME, "--ignored", "--exact", "--list"],
                                  root, environment, SELECTION_TIMEOUT, MAX_LIST_BYTES)
        validate_test_listing(listing)
        stage = "runtime"
        executed = bounded_process([str(executable), TEST_NAME, "--ignored", "--exact",
                                    "--test-threads=1", "--nocapture"], root, environment,
                                   RUNTIME_TIMEOUT, MAX_RUNTIME_BYTES)
        stage = "validation"
        require(before == [digest(path) for path in selected], "selected_binary_changed")
        require(not (evidence / "native-failure.json").exists(), "native_failure_with_success")
        receipt = validate_execution(executed)
        evidence.rmdir()
        print(PREFIX + json.dumps(receipt, ensure_ascii=True, sort_keys=True, allow_nan=False))
        print("File Structure acceptance passed: Linux fixture, 11 requests, local actions emitted none.")
        return receipt
    except (AcceptanceError, OSError, ValueError) as error:
        report_failure(evidence, stage)
        error.file_structure_failure_reported = True
        raise


def main():
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parent.parent)
    except AcceptanceError as error:
        if not getattr(error, "file_structure_failure_reported", False):
            report_failure(None, "setup")
        print("File Structure acceptance failed: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError) as error:
        if not getattr(error, "file_structure_failure_reported", False):
            report_failure(None, "setup")
        print("File Structure acceptance failed: driver_error", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
