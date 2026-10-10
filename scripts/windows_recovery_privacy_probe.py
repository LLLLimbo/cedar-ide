#!/usr/bin/env python3
"""Observe one nonshipping Windows inherited-descriptor feasibility test.

Compilation (120s) and exact-one selection (15s) precede one 60s watchdog
covering all native object work, descriptor reads, rename, and cleanup. A
candidate rejection is an observation; API, parser, or cleanup errors fail.
Only sanitized JSON is published. No ACL changes or recovery plaintext writes
are requested. Run with Python 3.12+ (nonblocking Windows pipes).
"""
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time

TEST_TARGET = "windows_privacy_probe"
TEST_NAME = "windows_default_inherited_descriptor_probe"
PREFIX = "CEDAR_RECOVERY_DESCRIPTOR_PROBE="
EVIDENCE_NAME = "WINDOWS_RECOVERY_PRIVACY_PROBE.json"
COMPILE_TIMEOUT = 120
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 60
REAP_TIMEOUT = 5
MAX_COMPILE_BYTES = 4 * 1024 * 1024
MAX_LIST_BYTES = 16 * 1024
MAX_PROBE_BYTES = 64 * 1024
COMMIT = re.compile(r"[0-9a-f]{40}")
ROOT_BOOLS = ("rename_attempted", "rename_completed", "cleanup_complete")
ROOT_STATES = ("identity_check", "rename_error_category")
ROOT_COUNTS = {
    "objects_observed": 5, "descriptor_reads": 5, "candidate_accepts": 5,
    "candidate_rejections": 5, "ace_count": 640, "allow_ace_count": 640,
    "deny_ace_count": 640, "inherited_ace_count": 640, "inherit_only_ace_count": 640,
}
OBJECT_ROLES = ("root", "child_directory", "lock", "temporary", "renamed_record")
ACE_COUNTS = ("ace_count", "allow_ace_count", "deny_ace_count",
              "inherited_ace_count", "inherit_only_ace_count")
REJECTIONS = {
    "owner_mismatch", "absent_dacl", "null_dacl", "empty_dacl", "unsupported_ace",
    "foreign_allow", "no_allow", "not_disk", "wrong_type", "reparse", "link_count",
    "persistent_acl_unavailable", "nonempty_file",
}
OWNER_CATEGORIES = {"effective_user", "system", "administrators", "other"}
DACL_REJECTIONS = {"absent_dacl", "null_dacl", "empty_dacl", "unsupported_ace",
                   "foreign_allow", "no_allow"}
OBJECT_REJECTIONS = REJECTIONS - DACL_REJECTIONS - {"owner_mismatch"}
RENAME_ERRORS = {
    "rename_access_denied", "rename_sharing_violation", "rename_lock_violation",
    "rename_not_supported", "rename_invalid_parameter", "rename_name_collision",
    "rename_path_missing", "rename_other",
}
# The native test supplies only fixed categories; this list is deliberately
# explicit so adding a raw native error string cannot silently publish it.
NATIVE_ERRORS = {
    "descriptor_limit", "malformed_descriptor", "malformed_sid", "malformed_acl", "ace_limit",
    "cleanup_failed", "admission_timeout", "root_environment", "object_open", "object_metadata",
    "identity_changed", "token_open", "token_query", "token_bounds", "descriptor_read",
    "unsafe_object", "root_create",
} | RENAME_ERRORS
DRIVER_ERRORS = {
    "duplicate_json_key", "nonfinite_json", "invalid_output_file", "output_identity_changed",
    "output_size_changed", "output_pipe_missing", "subprocess_timeout", "subprocess_output_limit",
    "subprocess_nonzero", "child_cleanup_unverified", "invalid_test_listing",
    "exact_one_test_missing", "native_receipt_invalid", "native_probe_error",
    "native_cleanup_or_identity_unverified", "native_observation_incomplete",
    "native_observation_inconsistent", "invalid_compiler_output", "invalid_compiler_event",
    "invalid_test_artifact", "exact_one_binary_missing", "unexpected_test_binary",
    "invalid_source_commit", "source_commit_mismatch", "source_checkout_dirty",
    "native_windows_required", "driver_cleanup_failed", "driver_error",
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


def decode_objects(root):
    objects = root["objects"]
    require(type(objects) is list and len(objects) <= len(OBJECT_ROLES), "native_receipt_invalid")
    decoded = []
    for item, role in zip(objects, OBJECT_ROLES):
        require(type(item) is dict and set(item) == set(ACE_COUNTS)
                | {"role", "outcome", "category", "owner_matches", "owner_category", "dacl_category"},
                "native_receipt_invalid")
        require(item["role"] == role and type(item["owner_matches"]) is bool,
                "native_receipt_invalid")
        require(type(item["owner_category"]) is str and item["owner_category"] in OWNER_CATEGORIES
                and item["owner_matches"] == (item["owner_category"] == "effective_user")
                and type(item["dacl_category"]) is str
                and item["dacl_category"] in DACL_REJECTIONS | {"accepted"}, "native_receipt_invalid")
        accepted = item["outcome"] == "candidate_accepted"
        require((accepted and item["category"] == "accepted" and item["owner_matches"]
                 and item["dacl_category"] == "accepted")
                or (item["outcome"] == "candidate_rejected" and type(item["category"]) is str
                    and (item["category"] in OBJECT_REJECTIONS
                         or (item["category"] == "owner_mismatch" and not item["owner_matches"])
                         or (item["category"] in DACL_REJECTIONS and item["owner_matches"]
                             and item["category"] == item["dacl_category"]))), "native_receipt_invalid")
        for key in ACE_COUNTS:
            require(type(item[key]) is int and 0 <= item[key] <= 128, "native_receipt_invalid")
        require(item["allow_ace_count"] + item["deny_ace_count"] <= item["ace_count"]
                and item["inherited_ace_count"] <= item["ace_count"]
                and item["inherit_only_ace_count"] <= item["ace_count"], "native_receipt_invalid")
        dacl = item["dacl_category"]
        if dacl in {"absent_dacl", "null_dacl", "empty_dacl"}:
            require(all(item[key] == 0 for key in ACE_COUNTS), "native_receipt_invalid")
        elif dacl in {"accepted", "foreign_allow"}:
            require(item["allow_ace_count"] > 0, "native_receipt_invalid")
        else:
            require(item["ace_count"] > 0, "native_receipt_invalid")
        if dacl in {"accepted", "no_allow"}:
            require(item["allow_ace_count"] + item["deny_ace_count"] == item["ace_count"],
                    "native_receipt_invalid")
        if dacl == "no_allow":
            require(item["allow_ace_count"] == 0, "native_receipt_invalid")
        decoded.append({key: item[key] for key in (*ACE_COUNTS, "role", "outcome",
                                                 "category", "owner_matches", "owner_category",
                                                 "dacl_category")})
    require(len(decoded) == root["objects_observed"]
            and sum(item["outcome"] == "candidate_accepted" for item in decoded) == root["candidate_accepts"]
            and sum(item["outcome"] == "candidate_rejected" for item in decoded) == root["candidate_rejections"],
            "native_receipt_invalid")
    for key in ACE_COUNTS:
        require(sum(item[key] for item in decoded) == root[key], "native_receipt_invalid")
    if root["outcome"] == "candidate_rejected":
        rejected = [item for item in decoded if item["outcome"] == "candidate_rejected"]
        require(rejected and root["category"] == rejected[0]["category"], "native_receipt_invalid")
    return decoded


def decode_probe(data):
    """Strictly reconstruct bounded evidence, including failed native runs."""
    try:
        require(type(data) is bytes and len(data) <= MAX_PROBE_BYTES, "native_receipt_invalid")
        text = data.decode("utf-8", errors="strict")
        require(text.count(PREFIX) == 1, "native_receipt_invalid")
        # libtest may put its 'test NAME ... ' immediately before println!.
        value = strict_json(text.split(PREFIX, 1)[1].splitlines()[0])
        require(type(value) is dict and set(value) == {
            "schema_version", "probe", "shipping_unchanged", "metadata_bytes_written",
            "body_bytes_written", "descriptor_query", "roots",
        }, "native_receipt_invalid")
        require(type(value["schema_version"]) is int and value["schema_version"] == 2
                and value["probe"] == TEST_NAME and value["shipping_unchanged"] is True
                and value["descriptor_query"] == "GetKernelObjectSecurity",
                "native_receipt_invalid")
        for key in ("metadata_bytes_written", "body_bytes_written"):
            require(type(value[key]) is int and value[key] == 0, "native_receipt_invalid")
        require(type(value["roots"]) is list and len(value["roots"]) == 2, "native_receipt_invalid")
        roots = []
        for root, label in zip(value["roots"], ("runner_temp", "local_app_data")):
            require(type(root) is dict and set(root) == set(ROOT_BOOLS) | set(ROOT_COUNTS) | set(ROOT_STATES)
                    | {"root", "outcome", "stage", "category", "objects"}, "native_receipt_invalid")
            require(root["root"] == label and root["outcome"] in (
                "candidate_accepted", "candidate_rejected", "error")
                and root["stage"] in ("setup", "root", "child_directory", "lock", "temporary", "rename",
                                      "renamed", "complete", "cleanup"), "native_receipt_invalid")
            require(type(root["category"]) is str and root["category"] in
                    REJECTIONS | NATIVE_ERRORS | {"accepted"}, "native_receipt_invalid")
            for key in ROOT_BOOLS:
                require(type(root[key]) is bool, "native_receipt_invalid")
            require(type(root["identity_check"]) is str
                    and root["identity_check"] in {"not_run", "stable", "changed"}
                    and type(root["rename_error_category"]) is str
                    and root["rename_error_category"] in RENAME_ERRORS | {"not_run", "none"},
                    "native_receipt_invalid")
            if not root["rename_attempted"]:
                require(not root["rename_completed"] and root["rename_error_category"] == "not_run"
                        and root["identity_check"] == "not_run", "native_receipt_invalid")
            elif root["rename_completed"]:
                require(root["rename_error_category"] == "none", "native_receipt_invalid")
            else:
                require(root["rename_error_category"] in RENAME_ERRORS
                        and root["identity_check"] == "not_run", "native_receipt_invalid")
            if root["identity_check"] == "changed":
                require(root["outcome"] == "error"
                        and root["category"] in {"identity_changed", "cleanup_failed"}, "native_receipt_invalid")
            if root["rename_error_category"] in RENAME_ERRORS:
                require(root["outcome"] == "error"
                        and root["category"] in {root["rename_error_category"], "cleanup_failed"},
                        "native_receipt_invalid")
            if root["category"] in RENAME_ERRORS:
                require(root["rename_error_category"] == root["category"], "native_receipt_invalid")
            for key, bound in ROOT_COUNTS.items():
                require(type(root[key]) is int and 0 <= root[key] <= bound, "native_receipt_invalid")
            require(root["candidate_accepts"] + root["candidate_rejections"] == root["objects_observed"]
                    <= root["descriptor_reads"], "native_receipt_invalid")
            require(root["allow_ace_count"] + root["deny_ace_count"] <= root["ace_count"]
                    and root["inherited_ace_count"] <= root["ace_count"]
                    and root["inherit_only_ace_count"] <= root["ace_count"], "native_receipt_invalid")
            decoded = {key: root[key] for key in (*ROOT_BOOLS, *ROOT_STATES, *ROOT_COUNTS,
                                                "root", "outcome", "stage", "category")}
            decoded["objects"] = decode_objects(root)
            require(not root["rename_attempted"] or len(decoded["objects"]) >= 4,
                    "native_receipt_invalid")
            require(len(decoded["objects"]) < 5 or root["identity_check"] == "stable",
                    "native_receipt_invalid")
            require(root["outcome"] != "error" or root["category"] in NATIVE_ERRORS,
                    "native_receipt_invalid")
            roots.append(decoded)
        return {"schema_version": 2, "probe": TEST_NAME, "shipping_unchanged": True,
                "metadata_bytes_written": 0, "body_bytes_written": 0,
                "descriptor_query": "GetKernelObjectSecurity", "roots": roots}
    except (ValueError, TypeError, KeyError, IndexError, RecursionError):
        raise ProbeError("native_receipt_invalid") from None


def validate_observation(receipt):
    for root in receipt["roots"]:
        require(root["outcome"] != "error", "native_probe_error")
        require(root["stage"] == "complete" and all(root[key] for key in ROOT_BOOLS)
                and root["identity_check"] == "stable" and root["rename_error_category"] == "none",
                "native_cleanup_or_identity_unverified")
        require(root["objects_observed"] == root["descriptor_reads"] == 5
                and root["candidate_accepts"] + root["candidate_rejections"] == 5,
                "native_observation_incomplete")
        accepted = root["outcome"] == "candidate_accepted"
        require((accepted and root["candidate_rejections"] == 0 and root["category"] == "accepted")
                or (not accepted and root["candidate_rejections"] > 0 and root["category"] in REJECTIONS),
                "native_observation_inconsistent")


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


def record_source_check(receipt, root, scratch, environment, source_commit, suffix):
    """Keep a check that did not run distinct from mismatch and command errors."""
    try:
        verify_source(root, scratch, environment, source_commit, suffix)
    except Exception as error:
        category = error_category(error)
        receipt["source_check_" + suffix] = (
            "mismatch" if category in {"source_commit_mismatch", "source_checkout_dirty"} else "error")
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


def run(root, scratch_root, source_commit):
    root = Path(root).resolve(strict=True)
    scratch_root = Path(scratch_root).resolve(strict=True)
    require(scratch_root.is_dir(), "scratch_root_missing")
    evidence = scratch_root / EVIDENCE_NAME
    require(not evidence.exists(), "receipt_already_exists")
    receipt = {
        "schema_version": 2, "kind": "windows_recovery_descriptor_feasibility",
        "status": "failed", "stage": "setup", "category": "driver_error",
        "source_commit": source_commit if type(source_commit) is str and COMMIT.fullmatch(source_commit) else None,
        "source_verified_before": False, "source_verified_after": False,
        "source_check_before": "not_run", "source_check_after": "not_run",
        "source_check_before_category": "not_run", "source_check_after_category": "not_run",
        "compile_timeout_s": COMPILE_TIMEOUT, "selection_timeout_s": SELECTION_TIMEOUT,
        "runtime_watchdog_s": RUNTIME_TIMEOUT, "child_reap_timeout_s": REAP_TIMEOUT,
        "exact_one_test_selected": False, "runtime_invocations": 0,
        "raw_logs_published": False, "driver_scratch_removed": False,
        "driver_cleanup_category": "not_run", "probe": None,
    }
    scratch = None
    runtime_log = None
    try:
        require(sys.platform == "win32", "native_windows_required")
        scratch = Path(tempfile.mkdtemp(prefix="cedar-recovery-probe-", dir=scratch_root)).resolve(strict=True)
        environment = os.environ.copy()
        environment["CARGO_TARGET_DIR"] = str(root / "target")
        environment["CARGO_TERM_COLOR"] = "never"
        environment.pop("RUST_TEST_THREADS", None)
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
        receipt["probe"] = decode_probe(read_regular(runtime_log, MAX_PROBE_BYTES))
        validate_observation(receipt["probe"])
        receipt.update(status="observed", stage="complete", category="none")
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
                    if receipt["status"] == "observed":
                        receipt.update(status="failed", stage="source_after", category=error_category(error))
            try:
                shutil.rmtree(scratch)
                receipt["driver_scratch_removed"] = not scratch.exists()
            except OSError:
                pass
            receipt["driver_cleanup_category"] = (
                "none" if receipt["driver_scratch_removed"] else "driver_cleanup_failed")
            if not receipt["driver_scratch_removed"] and receipt["status"] == "observed":
                receipt.update(status="failed", stage="cleanup", category="driver_cleanup_failed")
        publish(evidence, receipt)
    return receipt


def main():
    try:
        result = run(Path(__file__).resolve().parent.parent,
                     os.environ["RUNNER_TEMP"], os.environ.get("GITHUB_SHA"))
        return 0 if result["status"] == "observed" else 1
    except Exception:
        # This fallback is only for a missing/unwritable receipt destination.
        print('{"kind":"windows_recovery_descriptor_feasibility","status":"failed","category":"receipt_unavailable"}')
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
