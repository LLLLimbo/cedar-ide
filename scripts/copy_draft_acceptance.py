#!/usr/bin/env python3
"""CI-only Copy proof: compile, select, and execute one ignored libtest.

Compile (120 s), selection (15 s), and runtime (180 s) have separate bounds;
each driver child has a final 5 s reap. The Rust proof retains its own shared
115 s active / 120 s total deadline, including its owned fixture cleanup.
Requires Python 3.12 for nonblocking anonymous pipes on Windows. Only the
fixed, strictly validated receipts are printed; raw child output is bounded.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

TEST_TARGET = "cedar_app"
TEST_NAME = "copy_draft_process_tests::copy_to_new_draft_is_local_and_explicit_save_requires_absence"
PREFIX = "copy_draft_acceptance "
COMPILE_TIMEOUT = 120
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 180
REAP_TIMEOUT = 5
MAX_COMPILE_BYTES = 4 * 1024 * 1024
MAX_LIST_BYTES = 16 * 1024
MAX_RUNTIME_BYTES = 256 * 1024
SOURCE = "source Ω.txt"
DESTINATION = "copy café.txt"
CONTROLLED_PATH = "draft.txt"
BASE = "original α\nsecond café\nthird line\n"
DRAFT = "unsaved draft β\nsecond café\nthird line\n"
CASES = (
    "normal_dirty_explicit_save",
    "normal_preexisting_destination",
    "normal_presave_destination_race",
    "normal_source_changed_reject",
    "normal_reconnect_reject",
    "controlled_unknown_source_copy",
    "controlled_copy_save_unknown",
)


class AcceptanceError(ValueError):
    """A fixed failure category, without raw child output or fixture paths."""


def require(condition, category):
    if not condition:
        raise AcceptanceError(category)


def strict_json(data):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate_json_key")
            result[key] = value
        return result

    def nonfinite(_):
        raise AcceptanceError("nonfinite_json")

    try:
        return json.loads(data, object_pairs_hook=unique_object, parse_constant=nonfinite)
    except (json.JSONDecodeError, RecursionError):
        raise AcceptanceError("invalid_json") from None


def typed_equal(actual, expected):
    """JSON equality must not let bool equal int, even in a nested ledger."""
    if type(actual) is not type(expected):
        return False
    if type(expected) is dict:
        return (actual.keys() == expected.keys()
                and all(typed_equal(actual[key], value) for key, value in expected.items()))
    if type(expected) is list:
        return len(actual) == len(expected) and all(
            typed_equal(left, right) for left, right in zip(actual, expected))
    return actual == expected


def expected_receipts(platform):
    """Independent fixed contract, never populated from subprocess receipts."""
    require(platform in ("linux", "windows"), "unsupported_platform")
    base_digest = hashlib.sha256(BASE.encode("utf-8")).hexdigest()
    draft_digest = hashlib.sha256(DRAFT.encode("utf-8")).hexdigest()
    result = []
    for index, case in enumerate(CASES):
        source = CONTROLLED_PATH if index == 5 else SOURCE
        destination = CONTROLLED_PATH if index == 6 else DESTINATION
        hello = {"op": "Hello"}
        listing = {"op": "List", "path": ""}
        reading = {"op": "Read", "path": source}
        writing = {"op": "Write", "path": destination,
                   "expected_revision": None, "text_sha256": draft_digest}
        source_write = {"op": "Write", "path": source,
                        "expected_revision": base_digest, "text_sha256": draft_digest}
        ledgers = (
            [hello, listing, reading, writing, listing],
            [hello, listing, reading, writing],
            [hello, listing, reading, writing],
            [hello, listing, reading],
            [hello, listing, reading, hello, listing],
            [hello, listing, reading, source_write, hello, listing, writing, listing],
            [hello, listing, reading, hello, listing, writing],
        )
        ledger = ledgers[index]
        operations = [entry["op"] for entry in ledger]
        copies = index not in (3, 4)
        result.append({
            "schema": 1, "test": case, "os": platform,
            "controlled_peer": index >= 5,
            "controlled_fault": "synthetic-wrong-digest" if index >= 5 else None,
            "execution_trusted": False, "source": source, "destination": destination,
            "requests": len(ledger), "hello": operations.count("Hello"),
            "list": operations.count("List"), "read": operations.count("Read"),
            "write": operations.count("Write"), "source_writes": int(index == 5),
            "destination_writes": int(copies),
            "successful_ack_refresh_lists": int(index in (0, 5)),
            "connections": operations.count("Hello"), "reaped": operations.count("Hello"),
            "operation_ledger": ledger,
            "controlled_operation_ledger": (["Hello", "List", "Read", "Write"] if index == 5
                                            else ["Hello", "List", "Write"] if index == 6 else []),
            "copy_created": copies, "actual_modal_input": index == 0,
            "copy_requests": 0, "destination_prereads": 0, "run": 0, "language": 0,
            "absence_precondition_verified": copies,
            "source_state_preserved": True, "full_selection_preserved": True,
            "undo_redo_preserved": True, "original_recovery_bytes_preserved": True,
            "owned_recovery_verified": True, "source_disk_verified": True,
            "recovery_quiescence_verified": True, "recovery_workers_started": 1,
            "recovery_workers_stopped": 1,
            "destination_disk_verified": True, "no_replay": True,
            "unknown_source_retained": index == 5, "unknown_copy_retained": index == 6,
            "same_frame_source_edit_rejected": index == 3, "reconnect_rejected": index == 4,
            "presave_destination_race": index == 2, "post_preparation_commit_race": False,
            "shared_watchdog_seconds": 115, "total_budget_seconds": 120,
            "request_admission_seconds": 35, "recovery_admission_seconds": 10,
            "recovery_timeout_seconds": 5, "cleanup_timeout_seconds": 5,
            "owned_children_reaped": True, "fixtures_removed": True,
        })
    return result


def decode_output(data, limit):
    require(type(data) is bytes and len(data) <= limit, "invalid_output_size")
    try:
        return data.decode("utf-8", errors="strict")
    except UnicodeDecodeError:
        raise AcceptanceError("invalid_output_encoding") from None


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
    require(text.count(PREFIX) == len(CASES), "exact_seven_receipts_required")
    receipts = []
    for line in lines:
        if PREFIX not in line:
            continue
        before, value = line.split(PREFIX, 1)
        require(before in ("", "test " + TEST_NAME + " ... "), "invalid_receipt_prefix")
        receipts.append(strict_json(value))
    require(typed_equal(receipts, expected_receipts(platform)), "fixed_receipt_contract_mismatch")
    for key, total in {"requests": 35, "hello": 10, "list": 12, "read": 7, "write": 6,
                       "connections": 10, "reaped": 10, "successful_ack_refresh_lists": 2,
                       "recovery_workers_started": 7, "recovery_workers_stopped": 7}.items():
        require(sum(receipt[key] for receipt in receipts) == total, "aggregate_budget_mismatch")
    return receipts


def test_compile_command():
    return ["cargo", "test", "-p", "cedar-app", "--lib", "--locked", "--no-run",
            "--message-format=json"]


def compiled_test(data, root, platform):
    require(platform in ("linux", "windows"), "unsupported_platform")
    executables = []
    for line in decode_output(data, MAX_COMPILE_BYTES).splitlines():
        if not line.startswith("{"):
            continue  # Cargo also emits progress/diagnostic text on stderr.
        event = strict_json(line)
        require(type(event) is dict, "invalid_compiler_event")
        if event.get("reason") != "compiler-artifact":
            continue
        target = event.get("target")
        require(type(target) is dict, "invalid_compiler_target")
        if target.get("name") != TEST_TARGET:
            continue
        require(target.get("kind") == ["lib"]
                and type(event.get("profile")) is dict
                and event["profile"].get("test") is True,
                "invalid_test_artifact")
        require(event.get("manifest_path") == str(root / "crates/app/Cargo.toml")
                and target.get("src_path") == str(root / "crates/app/src/lib.rs"),
                "unexpected_test_source")
        executables.append(event.get("executable"))
    require(len(executables) == 1 and type(executables[0]) is str, "exact_one_binary_required")
    executable = Path(executables[0])
    suffix = r"\.exe" if platform == "windows" else ""
    require(executable.is_absolute()
            and executable.parent == root / "target/debug/deps"
            and re.fullmatch(TEST_TARGET + r"-[0-9a-f]+" + suffix, executable.name),
            "unexpected_test_binary")
    require(executable.is_file() and not executable.is_symlink()
            and executable.resolve(strict=True) == executable, "unexpected_test_binary")
    return executable


def validate_test_listing(data):
    lines = [line for line in decode_output(data, MAX_LIST_BYTES).splitlines() if line]
    require(lines == [TEST_NAME + ": test", "1 test, 0 benchmarks"], "exact_one_test_not_selected")


def bounded_process(command, root, environment, timeout, limit):
    """Bound output while draining; the deadline also covers EOF after exit.

    Only this directly owned child is killed/reaped on a driver failure. Native
    fixture/agent cleanup is established by the Rust receipts, never inferred
    from driver termination. No subprocess, selection, or runtime is retried.
    """
    started = time.monotonic()
    output = bytearray()
    process = subprocess.Popen(command, cwd=root, env=environment, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    try:
        require(process.stdout is not None, "output_pipe_missing")
        os.set_blocking(process.stdout.fileno(), False)
        open_pipe = True
        while open_pipe or process.poll() is None:
            require(time.monotonic() - started < timeout, "subprocess_timeout")
            chunk = None
            if open_pipe:
                try:
                    chunk = os.read(process.stdout.fileno(), min(65536, limit - len(output) + 1))
                except BlockingIOError:
                    pass
                if chunk == b"":
                    open_pipe = False
                elif chunk is not None:
                    require(len(chunk) <= limit - len(output), "subprocess_output_limit")
                    output.extend(chunk)
            if chunk is None:
                time.sleep(min(0.01, max(0, timeout - (time.monotonic() - started))))
        require(time.monotonic() - started < timeout, "subprocess_timeout")
        require(process.returncode == 0, "subprocess_nonzero")
        return bytes(output)
    finally:
        try:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=REAP_TIMEOUT)
        except (OSError, subprocess.TimeoutExpired):
            raise AcceptanceError("child_cleanup_unverified") from None
        finally:
            if process.stdout is not None:
                process.stdout.close()


def run(root):
    root = Path(root).resolve(strict=True)
    platform = {"linux": "linux", "win32": "windows"}.get(sys.platform)
    require(platform is not None, "unsupported_platform")
    suffix = ".exe" if platform == "windows" else ""
    environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
    for variable, name in (("CEDAR_COPY_DRAFT_AGENT_BIN", "cedar-agent"),
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
    print("Copy draft acceptance passed: 7 cases, 35 requests, 10 agents reaped.")
    return receipts


def main():
    try:
        require(len(sys.argv) == 1, "unexpected_arguments")
        run(Path(__file__).resolve().parent.parent)
    except AcceptanceError as error:
        print("Copy draft acceptance failed: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError):
        print("Copy draft acceptance failed: driver_error", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
