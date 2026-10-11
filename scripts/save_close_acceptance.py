#!/usr/bin/env python3
"""CI-only acknowledged Save and close proof; never retries a child or Write.

Compile (120 s), selection (15 s), and runtime (180 s) have separate bounds;
each directly owned child has a final 5 s reap. The Rust proof retains its own
115 s active / 120 s total deadline, including its owned fixture cleanup.
Requires Python 3.12 for nonblocking anonymous pipes on Windows. The shared
Copy driver provides bounded supervision and exact Cargo artifact selection.
Only fixed, strictly validated receipts are printed; raw output stays bounded.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import sys

from copy_draft_acceptance import (
    AcceptanceError,
    COMPILE_TIMEOUT,
    MAX_COMPILE_BYTES,
    MAX_LIST_BYTES,
    MAX_RUNTIME_BYTES,
    REAP_TIMEOUT,
    RUNTIME_TIMEOUT,
    SELECTION_TIMEOUT,
    bounded_process,
    compiled_test,
    decode_output,
    require,
    strict_json,
    test_compile_command,
    typed_equal,
)

TEST_NAME = "save_close_process_tests::save_and_close_is_acknowledged_bounded_and_targeted"
PREFIX = "save_close_acceptance "
SOURCE = "source Ω.txt"
CONTROLLED_PATH = "draft.txt"
OTHER = "other café.txt"
BASE = "original α\nsecond café\nthird line\n"
DRAFT = "unsaved draft β\nsecond café\nthird line\n"
NEWER = "newer source Ω\nsecond café\nthird line\n"
EXTERNAL = "independent on-disk change é\n"
CASES = (
    "normal_existing_valid_close",
    "normal_new_file_valid_close",
    "normal_conflict_retained",
    "controlled_wrong_digest_unknown_retained",
    "normal_cancel_after_dispatch_retained",
    "normal_newer_edits_before_ack_retained",
)


def expected_receipts(platform):
    """Independent fixed contract, never populated from subprocess receipts."""
    require(platform in ("linux", "windows"), "unsupported_platform")
    base_digest = hashlib.sha256(BASE.encode("utf-8")).hexdigest()
    draft_digest = hashlib.sha256(DRAFT.encode("utf-8")).hexdigest()
    newer_digest = hashlib.sha256(NEWER.encode("utf-8")).hexdigest()
    external_digest = hashlib.sha256(EXTERNAL.encode("utf-8")).hexdigest()
    result = []
    for index, case in enumerate(CASES):
        controlled = index == 3
        source = CONTROLLED_PATH if controlled else SOURCE
        closed = index in (0, 1)
        valid_ack = index not in (2, 3)
        dirty_source = index in (2, 3, 5)
        ledger = [{"session": 1, "op": "Hello"},
                  {"session": 1, "op": "List", "path": ""}]
        if index != 1:
            ledger.append({"session": 1, "op": "Read", "path": source})
        ledger.append({"session": 1, "op": "Write", "path": source,
                       "expected_revision": None if index == 1 else base_digest,
                       "text_sha256": draft_digest})
        if valid_ack:
            ledger.append({"session": 1, "op": "List", "path": ""})
        result.append({
            "schema": 1, "test": case, "os": platform,
            "controlled_peer": controlled,
            "controlled_fault": "synthetic-wrong-digest" if controlled else None,
            "execution_trusted": False, "source": source, "other": OTHER,
            "target_document": 1, "other_document": 2,
            "active_document_before": 2, "active_document_after": 2,
            "requests": len(ledger), "hello": 1, "list": 2 if valid_ack else 1,
            "read": int(index != 1), "write": 1, "connections": 1, "reaped": 1,
            "successful_ack_refresh_lists": int(valid_ack),
            "operation_ledger": ledger,
            "controlled_operation_ledger": ["Hello", "List", "Read", "Write"] if controlled else [],
            "session_ledger": [{"session": 1, "peer": "controlled" if controlled else "normal-release",
                                "hello": 1, "reaped": True}],
            "source_initial_sha256": None if index == 1 else base_digest,
            "submitted_sha256": draft_digest,
            "source_final_sha256": external_digest if index == 2 else draft_digest,
            "source_final_text": EXTERNAL if index == 2 else DRAFT,
            "ack_class": "conflict" if index == 2 else "wrong-digest" if controlled else "valid",
            "target_closed": closed, "target_retained": not closed,
            "target_text_after": None if closed else NEWER if index == 5 else DRAFT,
            "target_saved_text_after": None if closed else DRAFT if valid_ack else BASE,
            "target_revision_after": None if closed else draft_digest if valid_ack else base_digest,
            "close_before_ack": False, "new_file_absence_precondition": index == 1,
            "other_state_preserved": True, "other_recovery_bytes_preserved": True,
            "other_disk_absent": True, "full_selection_preserved": None if closed else True,
            "undo_redo_preserved": None if closed else True,
            "source_recovery_state": (
                "retained_newer_with_submitted_base" if index == 5
                else "retained_original_base" if dirty_source else "removed_after_valid_ack"),
            "source_recovery_text_sha256": (newer_digest if index == 5 else draft_digest)
            if dirty_source else None,
            "source_recovery_base_sha256": (draft_digest if index == 5 else base_digest)
            if dirty_source else None,
            "source_recovery_revision": (draft_digest if index == 5 else base_digest)
            if dirty_source else None,
            "recovery_quiescence_verified": True, "recovery_workers_started": 1,
            "recovery_workers_stopped": 1, "owned_recovery_verified": True,
            "no_replay": True, "no_workspace_file_delete": True, "run": 0, "language": 0,
            "shared_watchdog_seconds": 115, "total_budget_seconds": 120,
            "request_admission_seconds": 35, "recovery_admission_seconds": 10,
            "recovery_timeout_seconds": 5, "cleanup_timeout_seconds": 5,
            "owned_children_reaped": True, "fixtures_removed": True,
            "cancelled_after_dispatch": index == 4, "newer_edits_before_ack": index == 5,
            "baseline_adopted": valid_ack,
            "wrong_digest_unknown_retained": controlled, "source_disk_verified": True,
        })
    return result


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
    require(text.count(PREFIX) == len(CASES), "exact_six_receipts_required")
    receipts = []
    for line in lines:
        if PREFIX not in line:
            continue
        before, value = line.split(PREFIX, 1)
        require(before in ("", "test " + TEST_NAME + " ... "), "invalid_receipt_prefix")
        receipts.append(strict_json(value))
    require(typed_equal(receipts, expected_receipts(platform)), "fixed_receipt_contract_mismatch")
    for key, total in {"requests": 27, "hello": 6, "list": 10, "read": 5, "write": 6,
                       "connections": 6, "reaped": 6, "successful_ack_refresh_lists": 4,
                       "recovery_workers_started": 6, "recovery_workers_stopped": 6}.items():
        require(sum(receipt[key] for receipt in receipts) == total, "aggregate_budget_mismatch")
    return receipts


def validate_test_listing(data):
    lines = [line for line in decode_output(data, MAX_LIST_BYTES).splitlines() if line]
    require(lines == [TEST_NAME + ": test", "1 test, 0 benchmarks"], "exact_one_test_not_selected")


def run(root):
    root = Path(root).resolve(strict=True)
    platform = {"linux": "linux", "win32": "windows"}.get(sys.platform)
    require(platform is not None, "unsupported_platform")
    suffix = ".exe" if platform == "windows" else ""
    environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
    for variable, name in (("CEDAR_SAVE_CLOSE_AGENT_BIN", "cedar-agent"),
                           ("CEDAR_INTERRUPTED_SAVE_AGENT_BIN", "cedar-agent-interrupted-save-validation")):
        binary = root / "target/release" / (name + suffix)
        require(binary.is_file() and not binary.is_symlink()
                and binary.resolve(strict=True) == binary, "release_agent_missing")
        environment[variable] = str(binary)
    compiled = bounded_process(test_compile_command(), root, environment,
                               COMPILE_TIMEOUT, MAX_COMPILE_BYTES)
    executable = compiled_test(compiled, root, platform)
    selected = bounded_process([str(executable), TEST_NAME, "--ignored", "--exact", "--list"],
                               root, environment, SELECTION_TIMEOUT, MAX_LIST_BYTES)
    validate_test_listing(selected)
    executed = bounded_process([str(executable), TEST_NAME, "--ignored", "--exact",
                                "--test-threads=1", "--nocapture"],
                               root, environment, RUNTIME_TIMEOUT, MAX_RUNTIME_BYTES)
    receipts = validate_execution(executed, platform)
    for receipt in receipts:
        print(PREFIX + json.dumps(receipt, ensure_ascii=True, sort_keys=True, allow_nan=False))
    print("Save and close acceptance passed: 6 cases, 27 requests, 6 agents reaped.")
    return receipts


def main():
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parent.parent)
    except AcceptanceError as error:
        print("Save and close acceptance failed: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError):
        print("Save and close acceptance failed: driver_error", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
