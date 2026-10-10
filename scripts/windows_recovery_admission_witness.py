#!/usr/bin/env python3
"""Run one nonshipping Windows LocalAppData owner-rejection witness.

Compilation (120s), exact-one selection (15s), and one runtime (60s) are
separately bounded, with a 5s child reap. The source must match before and
after execution. Only fixed sanitized evidence is published. No retries,
alternate roots, prior descriptor probe, ACL repair, or recovery writes.
Requires Python 3.12+ for nonblocking Windows pipes.
"""
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import time
import tomllib

TEST_TARGET = "windows_recovery_admission"
TEST_NAME = "windows_local_app_data_owner_rejection_witness"
PREFIX = "CEDAR_RECOVERY_ADMISSION_WITNESS="
EVIDENCE_NAME = "WINDOWS_RECOVERY_ADMISSION_WITNESS.json"
COMPILE_TIMEOUT = 120
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 60
REAP_TIMEOUT = 5
MAX_COMPILE_BYTES = 4 * 1024 * 1024
MAX_LIST_BYTES = 16 * 1024
MAX_PROBE_BYTES = 64 * 1024
ACCEPTED_BASE = "676e713661b7112262f109b72251447521a1a7dc"
EXPECTED_VERSION = "0.46.0"
COMMIT = re.compile(r"[0-9a-f]{40}")
DRIVER_ERRORS = {
    "duplicate_json_key", "nonfinite_json", "invalid_output_file", "output_identity_changed",
    "output_size_changed", "output_pipe_missing", "subprocess_timeout", "subprocess_output_limit",
    "subprocess_nonzero", "child_cleanup_unverified", "invalid_test_listing",
    "exact_one_test_missing", "native_receipt_invalid", "native_probe_error",
    "native_cleanup_or_identity_unverified", "native_observation_incomplete",
    "native_observation_inconsistent", "invalid_compiler_output", "invalid_compiler_event",
    "invalid_test_artifact", "exact_one_binary_missing", "unexpected_test_binary",
    "invalid_source_commit", "source_commit_mismatch", "source_checkout_dirty",
    "source_parent_mismatch", "source_version_mismatch", "workflow_binding_mismatch",
    "native_windows_required", "driver_cleanup_failed", "driver_error",
    "invalid_test_execution", "exact_one_test_not_executed",
    "native_query_error", "native_intended_negative_fixture_unavailable",
    "native_unexpected_rejection", "native_setup_error", "native_cleanup_error", "native_opt_in_required",
}


class ProbeError(ValueError):
    """Messages are fixed categories, never external text."""


def error_category(error):
    category = str(error) if isinstance(error, ProbeError) else "driver_error"
    return category if category in DRIVER_ERRORS else "driver_error"


def require(condition, category):
    if not condition:
        raise ProbeError(category)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate_json_key")
        result[key] = value
    return result


def strict_json(data):
    def nonfinite(_):
        raise ProbeError("nonfinite_json")
    return json.loads(data, object_pairs_hook=unique_object, parse_constant=nonfinite)


def read_regular(path, limit):
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_size <= limit, "invalid_output_file")
    with path.open("rb") as stream:
        opened = os.fstat(stream.fileno())
        require((opened.st_dev, opened.st_ino) == (before.st_dev, before.st_ino),
                "output_identity_changed")
        data = stream.read(limit + 1)
    require(len(data) <= limit and len(data) == before.st_size, "output_size_changed")
    return data


def bounded_process(command, cwd, environment, log, timeout, limit):
    """Drain one owned child without reader threads or unbounded communicate.

    Python 3.12 supports nonblocking anonymous pipes on Windows. The same
    deadline covers process execution and pipe EOF, including after root exit.
    Direct-child termination/reaping is bounded separately and never proves
    native fixture cleanup (or compiler descendant cleanup).
    """
    started = time.monotonic()
    total = 0
    with log.open("xb") as stream:
        process = subprocess.Popen(command, cwd=cwd, env=environment,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT)
        try:
            require(process.stdout is not None, "output_pipe_missing")
            os.set_blocking(process.stdout.fileno(), False)
            open_pipe = True
            while open_pipe or process.poll() is None:
                require(time.monotonic() - started < timeout, "subprocess_timeout")
                chunk = None
                if open_pipe:
                    try:
                        chunk = os.read(process.stdout.fileno(), min(65536, limit - total + 1))
                    except BlockingIOError:
                        pass
                    if chunk == b"":
                        open_pipe = False
                    elif chunk is not None:
                        require(len(chunk) <= limit - total, "subprocess_output_limit")
                        stream.write(chunk)
                        total += len(chunk)
                if chunk is None:
                    time.sleep(min(0.01, max(0, timeout - (time.monotonic() - started))))
            require(time.monotonic() - started < timeout, "subprocess_timeout")
            require(process.returncode == 0, "subprocess_nonzero")
        finally:
            try:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=REAP_TIMEOUT)
            except (OSError, subprocess.TimeoutExpired):
                raise ProbeError("child_cleanup_unverified") from None
            finally:
                if process.stdout is not None:
                    process.stdout.close()


def validate_test_listing(data):
    require(type(data) is bytes and len(data) <= MAX_LIST_BYTES, "invalid_test_listing")
    lines = [line for line in data.decode("utf-8", errors="strict").splitlines() if line]
    require(lines == [TEST_NAME + ": test", "1 test, 0 benchmarks"], "exact_one_test_missing")


NATIVE_COUNTS = {
    "observation_attempts": 3, "observations_succeeded": 3, "admission_attempts": 1,
    "fake_read_calls": 1, "fake_payload_calls": 0, "fake_remove_calls": 0,
    "generated_directories": 1, "generated_files": 1,
}
NATIVE_LIMITATIONS = {"fixture_scope": "controlled_fresh_runner",
                      "atomic_directory_creation": False, "ancestor_namespace_verified": False}
NATIVE_BOOLS = ("handles_dropped_before_cleanup", "empty_verified", "cleanup_complete")
NATIVE_CATEGORIES = {
    "owner_mismatch", "query_error", "intended_negative_fixture_unavailable",
    "unexpected_rejection", "setup_error", "cleanup_error", "opt_in_required",
}


def decode_probe(data):
    """Reconstruct only the fixed sanitized witness schema, including failures."""
    try:
        require(type(data) is bytes and len(data) <= MAX_PROBE_BYTES, "native_receipt_invalid")
        text = data.decode("utf-8", errors="strict")
        require(text.count(PREFIX) == 1, "native_receipt_invalid")
        value = strict_json(text.split(PREFIX, 1)[1].splitlines()[0])
        require(type(value) is dict and set(value) == set(NATIVE_COUNTS) | set(NATIVE_BOOLS)
                | set(NATIVE_LIMITATIONS) | {"schema_version", "witness", "category"}, "native_receipt_invalid")
        require(type(value["schema_version"]) is int and value["schema_version"] == 1
                and value["witness"] == TEST_NAME and type(value["category"]) is str
                and value["category"] in NATIVE_CATEGORIES, "native_receipt_invalid")
        require(all(type(value[key]) is type(expected) and value[key] == expected
                    for key, expected in NATIVE_LIMITATIONS.items()), "native_receipt_invalid")
        for key, bound in NATIVE_COUNTS.items():
            require(type(value[key]) is int and 0 <= value[key] <= bound, "native_receipt_invalid")
        for key in NATIVE_BOOLS:
            require(type(value[key]) is bool, "native_receipt_invalid")
        require(value["observations_succeeded"] <= value["observation_attempts"]
                and value["admission_attempts"] <= value["observation_attempts"],
                "native_receipt_invalid")
        return {key: value[key] for key in ("schema_version", "witness", "category",
                                            *NATIVE_COUNTS, *NATIVE_BOOLS, *NATIVE_LIMITATIONS)}
    except (ValueError, TypeError, KeyError, IndexError, RecursionError):
        raise ProbeError("native_receipt_invalid") from None


def validate_observation(receipt):
    category = receipt["category"]
    require(category == "owner_mismatch", "native_" + category)
    require(all(receipt[key] == 0 for key in ("fake_read_calls", "fake_payload_calls", "fake_remove_calls")),
            "native_observation_inconsistent")
    require(all(receipt[key] for key in NATIVE_BOOLS), "native_cleanup_or_identity_unverified")
    require(all(receipt[key] == 1 for key in (
        "observation_attempts", "observations_succeeded", "admission_attempts",
        "generated_directories", "generated_files")), "native_observation_incomplete")


def validate_execution(data):
    """A receipt alone does not prove libtest executed exactly one test."""
    require(type(data) is bytes and len(data) <= MAX_PROBE_BYTES, "invalid_test_execution")
    lines = data.decode("utf-8", errors="strict").splitlines()
    running = [line for line in lines if line.startswith("running ")]
    results = [line for line in lines if line.startswith("test result:")]
    require(running == ["running 1 test"] and len(results) == 1
            and re.fullmatch(r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; "
                             r"[0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s", results[0]),
            "exact_one_test_not_executed")


def test_compile_command():
    return ["cargo", "test", "-p", "cedar-recovery", "--test", TEST_TARGET,
            "--locked", "--offline", "--no-run", "--message-format=json"]


def compiled_test(data, root):
    require(type(data) is bytes and len(data) <= MAX_COMPILE_BYTES, "invalid_compiler_output")
    executables = []
    for line in data.decode("utf-8", errors="strict").splitlines():
        if not line.startswith("{"):
            continue
        event = strict_json(line)
        require(type(event) is dict, "invalid_compiler_event")
        if event.get("reason") == "compiler-artifact" and event.get("target", {}).get("name") == TEST_TARGET:
            require(event.get("target", {}).get("kind") == ["test"]
                    and event.get("profile", {}).get("test") is True,
                    "invalid_test_artifact")
            executables.append(event.get("executable"))
    require(len(executables) == 1 and type(executables[0]) is str, "exact_one_binary_missing")
    executable = Path(executables[0]).resolve(strict=True)
    require(executable.parent == (root / "target/debug/deps").resolve(strict=True)
            and re.fullmatch(TEST_TARGET + r"-[0-9a-f]+\.exe", executable.name)
            and executable.is_file(), "unexpected_test_binary")
    return executable


def verify_source(root, scratch, environment, source_commit, suffix):
    require(type(source_commit) is str and COMMIT.fullmatch(source_commit), "invalid_source_commit")
    head = scratch / ("head-" + suffix + ".log")
    status = scratch / ("status-" + suffix + ".log")
    bounded_process(["git", "rev-parse", "--verify", "HEAD"], root, environment, head, 10, 128)
    require(read_regular(head, 128).decode("ascii").strip() == source_commit, "source_commit_mismatch")
    bounded_process(["git", "status", "--porcelain", "--untracked-files=normal"],
                    root, environment, status, 10, MAX_PROBE_BYTES)
    require(not read_regular(status, MAX_PROBE_BYTES), "source_checkout_dirty")
    parent = scratch / ("parent-" + suffix + ".log")
    bounded_process(["git", "rev-parse", "--verify", "HEAD^"], root, environment, parent, 10, 128)
    require(read_regular(parent, 128).decode("ascii").strip() == ACCEPTED_BASE, "source_parent_mismatch")
    manifest = tomllib.loads(read_regular(root / "Cargo.toml", MAX_PROBE_BYTES).decode("utf-8"))
    require(manifest.get("workspace", {}).get("package", {}).get("version") == EXPECTED_VERSION,
            "source_version_mismatch")


def record_source_check(receipt, root, scratch, environment, source_commit, suffix):
    """Keep a check that did not run distinct from mismatch and command errors."""
    try:
        verify_source(root, scratch, environment, source_commit, suffix)
    except Exception as error:
        category = error_category(error)
        receipt["source_check_" + suffix] = (
            "mismatch" if category in {"source_commit_mismatch", "source_checkout_dirty",
                                        "source_parent_mismatch", "source_version_mismatch"} else "error")
        receipt["source_check_" + suffix + "_category"] = category
        raise
    receipt["source_verified_" + suffix] = True
    receipt["source_check_" + suffix] = "matched"
    receipt["source_check_" + suffix + "_category"] = "none"


def publish(path, result):
    # This fixed artifact contains only fields reconstructed by this driver.
    # No exception string, command, path, SID, descriptor, or subprocess log.
    data = (json.dumps(result, sort_keys=True, allow_nan=False) + "\n").encode("utf-8")
    require(len(data) <= MAX_PROBE_BYTES, "receipt_output_limit")
    with path.open("xb") as stream:
        stream.write(data)
    print(data.decode("utf-8"), end="")


def remove_driver_scratch(scratch):
    """Remove only the driver's fixed log names, never recurse or follow entries."""
    for name in ("head-before.log", "status-before.log", "parent-before.log",
                 "head-after.log", "status-after.log", "parent-after.log",
                 "compile.log", "selection.log", "runtime.log"):
        path = scratch / name
        try:
            path.unlink()
        except FileNotFoundError:
            pass
    scratch.rmdir()


def verify_workflow(environment, source_commit):
    require(environment.get("GITHUB_SHA") == source_commit, "workflow_binding_mismatch")
    require(all(environment.get(key) == value for key, value in {
        "GITHUB_REPOSITORY": "LLLLimbo/cedar-ide",
        "GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/heads/main",
        "GITHUB_RUN_ATTEMPT": "1",
    }.items()), "workflow_binding_mismatch")
    event_path = environment.get("GITHUB_EVENT_PATH")
    require(type(event_path) is str and bool(event_path), "workflow_binding_mismatch")
    event = strict_json(read_regular(Path(event_path), MAX_COMPILE_BYTES))
    require(type(event) is dict and event.get("before") == ACCEPTED_BASE,
            "workflow_binding_mismatch")


def run(root, scratch_root, source_commit):
    root = Path(root).resolve(strict=True)
    scratch_root = Path(scratch_root).resolve(strict=True)
    require(scratch_root.is_dir(), "scratch_root_missing")
    evidence = scratch_root / EVIDENCE_NAME
    require(not evidence.exists(), "receipt_already_exists")
    receipt = {
        "schema_version": 1, "kind": "windows_recovery_admission_witness",
        "status": "failed", "stage": "setup", "category": "driver_error",
        "source_commit": source_commit if type(source_commit) is str and COMMIT.fullmatch(source_commit) else None,
        "accepted_base": ACCEPTED_BASE, "expected_version": EXPECTED_VERSION,
        "source_verified_before": False, "source_verified_after": False,
        "source_check_before": "not_run", "source_check_after": "not_run",
        "source_check_before_category": "not_run", "source_check_after_category": "not_run",
        "compile_timeout_s": COMPILE_TIMEOUT, "selection_timeout_s": SELECTION_TIMEOUT,
        "runtime_watchdog_s": RUNTIME_TIMEOUT, "child_reap_timeout_s": REAP_TIMEOUT,
        "exact_one_test_selected": False, "exact_one_test_executed": False, "runtime_invocations": 0,
        "raw_logs_published": False, "driver_scratch_removed": False,
        "driver_cleanup_category": "not_run", "probe": None,
    }
    scratch = None
    runtime_log = None
    try:
        require(sys.platform == "win32", "native_windows_required")
        scratch = Path(tempfile.mkdtemp(prefix="cedar-recovery-admission-", dir=scratch_root)).resolve(strict=True)
        environment = os.environ.copy()
        verify_workflow(environment, source_commit)
        environment["CARGO_TARGET_DIR"] = str(root / "target")
        environment["CARGO_TERM_COLOR"] = "never"
        environment.pop("RUST_TEST_THREADS", None)
        environment["CEDAR_RUN_ADMISSION_WITNESS"] = "1"
        receipt["stage"] = "source_before"
        record_source_check(receipt, root, scratch, environment, source_commit, "before")
        receipt["stage"] = "compile"
        compile_log = scratch / "compile.log"
        bounded_process(test_compile_command(), root, environment, compile_log,
                        COMPILE_TIMEOUT, MAX_COMPILE_BYTES)
        executable = compiled_test(read_regular(compile_log, MAX_COMPILE_BYTES), root)
        receipt["stage"] = "selection"
        selection_log = scratch / "selection.log"
        bounded_process([str(executable), "--list", "--ignored", "--exact", TEST_NAME],
                        scratch, environment, selection_log, SELECTION_TIMEOUT, MAX_LIST_BYTES)
        validate_test_listing(read_regular(selection_log, MAX_LIST_BYTES))
        receipt["exact_one_test_selected"] = True
        receipt["stage"] = "runtime"
        runtime_log = scratch / "runtime.log"
        receipt["runtime_invocations"] = 1
        bounded_process([str(executable), "--ignored", "--exact", TEST_NAME,
                         "--nocapture", "--test-threads=1"], scratch, environment,
                        runtime_log, RUNTIME_TIMEOUT, MAX_PROBE_BYTES)
        receipt["stage"] = "receipt"
        runtime_data = read_regular(runtime_log, MAX_PROBE_BYTES)
        receipt["probe"] = decode_probe(runtime_data)
        validate_execution(runtime_data)
        receipt["exact_one_test_executed"] = True
        validate_observation(receipt["probe"])
        receipt.update(status="passed", stage="complete", category="none")
    except Exception as error:
        # A failed native process can still supply useful sanitized evidence.
        # It must never become an observation merely because JSON was emitted.
        if runtime_log is not None and receipt["probe"] is None:
            try:
                receipt["probe"] = decode_probe(read_regular(runtime_log, MAX_PROBE_BYTES))
            except (OSError, ValueError, TypeError):
                pass
        receipt["category"] = error_category(error)
    finally:
        if scratch is not None:
            # This independently bounded check runs after every attempted native
            # invocation, including timeout/nonzero/receipt failure. A later
            # source or cleanup failure must not erase the primary failure.
            if receipt["runtime_invocations"] == 1:
                try:
                    record_source_check(receipt, root, scratch, environment, source_commit, "after")
                except Exception as error:
                    if receipt["status"] == "passed":
                        receipt.update(status="failed", stage="source_after", category=error_category(error))
            try:
                remove_driver_scratch(scratch)
                receipt["driver_scratch_removed"] = not scratch.exists()
            except OSError:
                pass
            receipt["driver_cleanup_category"] = (
                "none" if receipt["driver_scratch_removed"] else "driver_cleanup_failed")
            if not receipt["driver_scratch_removed"] and receipt["status"] == "passed":
                receipt.update(status="failed", stage="cleanup", category="driver_cleanup_failed")
        publish(evidence, receipt)
    return receipt


def main():
    try:
        result = run(Path(__file__).resolve().parent.parent,
                     os.environ["RUNNER_TEMP"], os.environ.get("GITHUB_SHA"))
        return 0 if result["status"] == "passed" else 1
    except Exception:
        # This fallback is only for a missing/unwritable receipt destination.
        print('{"kind":"windows_recovery_admission_witness","status":"failed","category":"receipt_unavailable"}')
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
