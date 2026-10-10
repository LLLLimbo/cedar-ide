#!/usr/bin/env python3
"""Portable pure checks: no Cargo build, native execution, or prior probe launch."""
from contextlib import redirect_stdout
import copy
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import windows_recovery_admission_witness as probe


def listing():
    return (probe.TEST_NAME + ": test\r\n\r\n1 test, 0 benchmarks\r\n").encode()


def artifact(executable):
    return {"reason": "compiler-artifact", "target": {"name": probe.TEST_TARGET, "kind": ["test"]},
            "profile": {"test": True}, "executable": str(executable)}


def valid():
    return {"schema_version": 1, "witness": probe.TEST_NAME, "category": "owner_mismatch",
            **{key: (0 if key.startswith("fake_") else 1) for key in probe.NATIVE_COUNTS},
            **{key: True for key in probe.NATIVE_BOOLS}, **probe.NATIVE_LIMITATIONS}


def payload(receipt):
    return ("running 1 test\r\ntest " + probe.TEST_NAME + " ... " + probe.PREFIX
            + json.dumps(receipt) + "\r\nok\r\n\r\ntest result: ok. 1 passed; 0 failed; 0 ignored; "
            "0 measured; 5 filtered out; finished in 0.01s\r\n").encode()


class ReceiptTests(unittest.TestCase):
    def test_success_is_exact_observed_owner_rejection(self):
        decoded = probe.decode_probe(payload(valid()))
        self.assertEqual(decoded, valid())
        probe.validate_observation(decoded)
        probe.validate_execution(payload(valid()))

    def test_all_other_categories_are_explicit_not_pass(self):
        for category in probe.NATIVE_CATEGORIES - {"owner_mismatch"}:
            receipt = {**valid(), "category": category}
            self.assertEqual(probe.decode_probe(payload(receipt)), receipt)
            with self.subTest(category=category), self.assertRaises(probe.ProbeError):
                probe.validate_observation(receipt)

    def test_missing_extra_duplicate_malformed_or_raw_data_rejected(self):
        for data in (b"", payload(valid()) * 2, b"\xff", payload({**valid(), "path": "private"}),
                     payload({**valid(), "category": "private SID"}),
                     payload({**valid(), "schema_version": True}),
                     (probe.PREFIX + '{"schema_version":1,"schema_version":1}').encode(),
                     (probe.PREFIX + '{"schema_version":NaN}').encode(),
                     b"x" * (probe.MAX_PROBE_BYTES + 1)):
            with self.subTest(data=data[:30]), self.assertRaises(probe.ProbeError):
                probe.decode_probe(data)
        for key in valid():
            receipt = valid()
            del receipt[key]
            with self.subTest(key=key), self.assertRaises(probe.ProbeError):
                probe.decode_probe(payload(receipt))

    def test_all_numeric_and_boolean_types_and_bounds_are_strict(self):
        for key, bound in probe.NATIVE_COUNTS.items():
            for bad in (True, False, -1, bound + 1, 0.0, "0", None, []):
                with self.subTest(key=key, bad=bad), self.assertRaises(probe.ProbeError):
                    probe.decode_probe(payload({**valid(), key: bad}))
        for key in probe.NATIVE_BOOLS:
            for bad in (0, 1, "true", None):
                with self.subTest(key=key, bad=bad), self.assertRaises(probe.ProbeError):
                    probe.decode_probe(payload({**valid(), key: bad}))

    def test_limitations_cannot_be_removed_or_changed(self):
        for key in probe.NATIVE_LIMITATIONS:
            for bad in (None, 0, 1, True, "other"):
                with self.subTest(key=key, bad=bad), self.assertRaises(probe.ProbeError):
                    probe.decode_probe(payload({**valid(), key: bad}))

    def test_partial_observation_or_cleanup_never_passes(self):
        for key in (*probe.NATIVE_BOOLS, "observation_attempts", "observations_succeeded",
                    "admission_attempts", "generated_directories", "generated_files"):
            receipt = {**valid(), key: False if key in probe.NATIVE_BOOLS else 0}
            with self.subTest(key=key), self.assertRaises(probe.ProbeError):
                probe.validate_observation(probe.decode_probe(payload(receipt)))
        for changes in ({"observation_attempts": 2}, {"fake_read_calls": 1}):
            with self.assertRaises(probe.ProbeError):
                probe.validate_observation(probe.decode_probe(payload({**valid(), **changes})))

    def test_receipt_without_exact_one_executed_test_is_not_enough(self):
        good = payload(valid())
        for data in (good.replace(b"running 1 test", b"running 0 tests"),
                     good.replace(b"1 passed", b"0 passed"), good.replace(b"0 failed", b"1 failed"),
                     good.replace(b"0 ignored", b"1 ignored"), good + good,
                     (probe.PREFIX + json.dumps(valid())).encode(),
                     good.replace(b"test result: ok.", b"test result: FAILED.")):
            with self.subTest(data=data[:30]), self.assertRaises(probe.ProbeError):
                probe.validate_execution(data)


class SelectionTests(unittest.TestCase):
    def test_exact_ignored_test_and_crlf_listing(self):
        probe.validate_test_listing(listing())
        probe.validate_test_listing(listing().replace(b"\n", b"\r\n"))
        self.assertEqual(probe.test_compile_command(), [
            "cargo", "test", "-p", "cedar-recovery", "--test", "windows_recovery_admission",
            "--locked", "--offline", "--no-run", "--message-format=json"])

    def test_zero_wrong_duplicate_extra_and_malformed_listing_rejected(self):
        for data in (b"", b"0 tests, 0 benchmarks\n", b"wrong: test\n\n1 test, 0 benchmarks\n",
                     listing() + listing(), listing().replace(b"1 test", b"2 tests"),
                     b"extra: test\n" + listing(), listing() + b"private output\n", b"\xff",
                     b"x" * (probe.MAX_LIST_BYTES + 1)):
            with self.subTest(data=data[:60]), self.assertRaises(ValueError):
                probe.validate_test_listing(data)

    def test_one_exact_known_windows_binary_required(self):
        with tempfile.TemporaryDirectory(prefix="cedar-probe-selection-") as directory:
            root = Path(directory).resolve(strict=True)
            deps = root / "target/debug/deps"
            deps.mkdir(parents=True)
            executable = deps / "windows_recovery_admission-123abc.exe"
            executable.write_bytes(b"synthetic binary, never executed")
            event = artifact(executable)
            self.assertEqual(probe.compiled_test(json.dumps(event).encode(), root), executable)
            bad = [[], [event, event], [{**event, "executable": None}],
                   [{**event, "target": {"name": probe.TEST_TARGET, "kind": ["bin"]}}],
                   [{**event, "profile": {"test": 1}}]]
            for events in bad:
                with self.subTest(events=events), self.assertRaises(ValueError):
                    probe.compiled_test(b"\n".join(json.dumps(value).encode() for value in events), root)
            for path in (root / executable.name, deps / "other-123abc.exe",
                         deps / "windows_recovery_admission-123abc.dll"):
                path.write_bytes(b"synthetic")
                with self.subTest(name=path.name), self.assertRaises(ValueError):
                    probe.compiled_test(json.dumps(artifact(path)).encode(), root)


class SupervisorTests(unittest.TestCase):
    def exercise(self, chunks, *, returncode=0, timeout=10, limit=64, cleanup_error=None):
        process = mock.Mock()
        process.stdout.fileno.return_value = 42
        process.returncode = returncode
        process.poll.side_effect = lambda: process.returncode
        process.kill.side_effect = lambda: setattr(process, "returncode", -9)
        process.wait.side_effect = cleanup_error
        tick = [0.0]

        def now():
            tick[0] += 0.01
            return tick[0]

        def read(_fd, count):
            value = chunks.pop(0) if chunks else BlockingIOError()
            if isinstance(value, Exception):
                raise value
            self.assertLessEqual(len(value), count)
            return value

        with tempfile.TemporaryDirectory(prefix="cedar-probe-supervisor-") as directory:
            root = Path(directory).resolve(strict=True)
            log = root / "private.log"
            error = None
            with mock.patch.object(probe.subprocess, "Popen", return_value=process) as launch, \
                    mock.patch.object(probe.os, "set_blocking") as blocking, \
                    mock.patch.object(probe.os, "read", side_effect=read), \
                    mock.patch.object(probe.time, "monotonic", side_effect=now), \
                    mock.patch.object(probe.time, "sleep"):
                try:
                    probe.bounded_process(["owned synthetic child"], root, {}, log, timeout, limit)
                except ValueError as caught:
                    error = str(caught)
            self.assertEqual(launch.call_count, 1)
            blocking.assert_called_once_with(42, False)
            process.stdout.close.assert_called_once()
            process.wait.assert_called_once_with(timeout=5)
            return log.read_bytes(), error, process

    def test_complete_lf_bytes_are_drained_and_owned_child_reaped(self):
        data, error, process = self.exercise([BlockingIOError(), b"synthetic\n", b""])
        self.assertEqual(data, b"synthetic\n")
        self.assertIsNone(error)
        process.kill.assert_not_called()

    def test_output_never_exceeds_limit(self):
        data, error, process = self.exercise([b"x" * 8, b"x"], limit=8, returncode=None)
        self.assertEqual(data, b"x" * 8)
        self.assertEqual(error, "subprocess_output_limit")
        process.kill.assert_called_once()

    def test_one_watchdog_covers_running_child_and_pipe_after_exit(self):
        for returncode in (None, 0):
            with self.subTest(returncode=returncode):
                _, error, process = self.exercise([], returncode=returncode, timeout=0.1)
                self.assertEqual(error, "subprocess_timeout")
                self.assertEqual(process.kill.call_count, int(returncode is None))

    def test_nonzero_and_failed_reap_are_not_success(self):
        _, error, _ = self.exercise([b""], returncode=7)
        self.assertEqual(error, "subprocess_nonzero")
        _, error, _ = self.exercise([b""], cleanup_error=subprocess.TimeoutExpired("synthetic", 5))
        self.assertEqual(error, "child_cleanup_unverified")


class DriverTests(unittest.TestCase):
    def exercise(self, *, fail_stage=None, native=None, selection=None, source_change=False,
                 dirty=False, dirty_after=False, cleanup_failure=False, after_check_error=None,
                 wrong_parent=False, wrong_version=False, wrong_sha=False,
                 workflow_override=None, wrong_event_before=False,
                 failure_category="subprocess_timeout", native_data=None):
        native = valid() if native is None else native
        with tempfile.TemporaryDirectory(prefix="cedar-probe-driver-") as directory:
            # Resolve the owned root before deriving every path. In particular,
            # Windows temp aliases must not make an equality test spuriously fail.
            owned = Path(directory).resolve(strict=True)
            root = owned / "checkout"
            scratch_root = owned / "runner-temp"
            scratch_root.mkdir()
            deps = root / "target/debug/deps"
            deps.mkdir(parents=True)
            executable = deps / "windows_recovery_admission-123abc.exe"
            executable.write_bytes(b"synthetic binary, never executed")
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "' +
                                             ("0.45.0" if wrong_version else "0.46.0") + '"\n')
            event_path = owned / "event.json"
            event_path.write_text(json.dumps({"before": "b" * 40 if wrong_event_before else probe.ACCEPTED_BASE}))
            environment = {"GITHUB_REPOSITORY": "LLLLimbo/cedar-ide", "GITHUB_EVENT_NAME": "push",
                           "GITHUB_REF": "refs/heads/main", "GITHUB_RUN_ATTEMPT": "1",
                           "GITHUB_EVENT_PATH": str(event_path), "GITHUB_SHA": "a" * 40}
            environment.update(workflow_override or {})
            calls = []
            seen_heads = 0

            def run(command, cwd, environment, log, timeout, limit):
                nonlocal seen_heads
                calls.append((command, timeout, limit))
                if command == ["git", "rev-parse", "--verify", "HEAD^"]:
                    data = (("b" * 40 if wrong_parent else probe.ACCEPTED_BASE) + "\r\n").encode()
                elif command[:2] == ["git", "rev-parse"]:
                    seen_heads += 1
                    data = (("b" if wrong_sha or (source_change and seen_heads == 2) else "a") * 40 + "\n").encode()
                elif command[:2] == ["git", "status"]:
                    data = b" M private-path-never-published\n" if dirty or (dirty_after and seen_heads == 2) else b""
                elif command[0] == "cargo":
                    self.assertEqual(environment["CARGO_TARGET_DIR"], str(root / "target"))
                    data = json.dumps(artifact(executable)).encode() + b"\n"
                elif "--list" in command:
                    data = listing() if selection is None else selection
                else:
                    self.assertEqual(command, [str(executable), "--ignored", "--exact", probe.TEST_NAME,
                                               "--nocapture", "--test-threads=1"])
                    self.assertEqual(environment["CEDAR_RUN_ADMISSION_WITNESS"], "1")
                    data = payload(native) if native_data is None else native_data
                log.write_bytes(data)
                stage = "compile" if command[0] == "cargo" else (
                    "selection" if "--list" in command else "runtime" if command[0] == str(executable)
                    else "source_after" if seen_heads == 2 else "source_before")
                if stage == fail_stage:
                    raise probe.ProbeError(failure_category)
                if stage == "source_after" and after_check_error is not None:
                    raise after_check_error

            capture = io.StringIO()
            remove = probe.remove_driver_scratch

            def cleanup(path):
                if cleanup_failure:
                    raise OSError("private cleanup message")
                remove(path)

            with mock.patch.object(probe.sys, "platform", "win32"), \
                    mock.patch.object(probe, "bounded_process", side_effect=run), \
                    mock.patch.object(probe, "remove_driver_scratch", side_effect=cleanup), \
                    mock.patch.dict(probe.os.environ, environment, clear=True), redirect_stdout(capture):
                result = probe.run(root, scratch_root, "a" * 40)
            receipt = json.loads((scratch_root / probe.EVIDENCE_NAME).read_bytes())
            self.assertEqual(result, receipt)
            self.assertEqual(json.loads(capture.getvalue()), receipt)
            if not cleanup_failure:
                self.assertEqual(set(path.name for path in scratch_root.iterdir()), {probe.EVIDENCE_NAME})
            self.assertEqual(receipt["driver_scratch_removed"], not cleanup_failure)
            self.assertEqual(receipt["driver_cleanup_category"], "driver_cleanup_failed" if cleanup_failure else "none")
            self.assertNotIn(str(owned), capture.getvalue())
            self.assertNotIn("private-path", capture.getvalue())
            return receipt, calls

    def test_one_runtime_and_exact_source_bound_success(self):
        receipt, calls = self.exercise()
        self.assertEqual(receipt["status"], "passed")
        self.assertEqual(receipt["schema_version"], 1)
        self.assertTrue(receipt["source_verified_before"] and receipt["source_verified_after"])
        self.assertTrue(receipt["exact_one_test_selected"] and receipt["exact_one_test_executed"])
        self.assertEqual(receipt["runtime_invocations"], 1)
        self.assertEqual(receipt["probe"], valid())
        self.assertEqual([timeout for _, timeout, _ in calls], [10, 10, 10, 120, 15, 60, 10, 10, 10])
        self.assertEqual(probe.test_compile_command(), ["cargo", "test", "-p", "cedar-recovery", "--test",
            "windows_recovery_admission", "--locked", "--offline", "--no-run", "--message-format=json"])
        self.assertFalse(any("windows_privacy_probe" in str(command) for command, _, _ in calls))

    def test_wrong_source_bindings_launch_no_runtime(self):
        for changes, category in (({"wrong_sha": True}, "source_commit_mismatch"),
                                  ({"wrong_parent": True}, "source_parent_mismatch"),
                                  ({"wrong_version": True}, "source_version_mismatch"),
                                  ({"dirty": True}, "source_checkout_dirty")):
            receipt, calls = self.exercise(**changes)
            with self.subTest(changes=changes):
                self.assertEqual(receipt["status"], "failed")
                self.assertEqual(receipt["category"], category)
                self.assertEqual(receipt["runtime_invocations"], 0)
                self.assertEqual(receipt["source_check_after"], "not_run")
                self.assertFalse(any(command[0] == "cargo" for command, _, _ in calls))

    def test_wrong_workflow_bindings_launch_no_runtime(self):
        for key, bad in (("GITHUB_REPOSITORY", "other/repo"), ("GITHUB_EVENT_NAME", "pull_request"),
                         ("GITHUB_REF", "refs/heads/other"), ("GITHUB_RUN_ATTEMPT", "2"), ("GITHUB_SHA", "b" * 40)):
            receipt, calls = self.exercise(workflow_override={key: bad})
            with self.subTest(key=key):
                self.assertEqual(receipt["category"], "workflow_binding_mismatch")
                self.assertEqual(receipt["runtime_invocations"], 0)
                self.assertEqual(calls, [])
        receipt, calls = self.exercise(wrong_event_before=True)
        self.assertEqual(receipt["category"], "workflow_binding_mismatch")
        self.assertEqual(calls, [])

    def test_preparation_or_selection_failure_never_starts_runtime(self):
        for stage in ("compile", "selection"):
            receipt, _ = self.exercise(fail_stage=stage)
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["runtime_invocations"], 0)
        receipt, _ = self.exercise(selection=b"0 tests, 0 benchmarks\r\n")
        self.assertEqual(receipt["category"], "exact_one_test_missing")
        self.assertEqual(receipt["runtime_invocations"], 0)

    def test_failed_runtime_never_retried_and_source_still_checked(self):
        for category in ("subprocess_timeout", "subprocess_nonzero", "subprocess_output_limit",
                         "child_cleanup_unverified"):
            receipt, calls = self.exercise(fail_stage="runtime", failure_category=category)
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["category"], category)
            self.assertEqual(receipt["probe"], valid())
            self.assertEqual(receipt["source_check_after"], "matched")
            self.assertEqual(sum(timeout == 60 for _, timeout, _ in calls), 1)

    def test_query_error_and_unexpected_acceptance_are_explicit_not_pass(self):
        for category in probe.NATIVE_CATEGORIES - {"owner_mismatch"}:
            native = {**valid(), "category": category}
            if category == "query_error":
                native.update(observations_succeeded=0)
            if category == "intended_negative_fixture_unavailable":
                native.update(observation_attempts=3, observations_succeeded=3, fake_read_calls=1)
            receipt, calls = self.exercise(native=native)
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["category"], "native_" + category)
            self.assertEqual(receipt["probe"], native)
            self.assertEqual(sum(timeout == 60 for _, timeout, _ in calls), 1)

    def test_execution_count_missing_or_wrong_never_passes(self):
        for data in (payload(valid()).replace(b"1 passed", b"0 passed"),
                     (probe.PREFIX + json.dumps(valid())).encode()):
            receipt, _ = self.exercise(native_data=data)
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["category"], "exact_one_test_not_executed")
            self.assertFalse(receipt["exact_one_test_executed"])

    def test_native_cleanup_and_partial_observation_do_not_pass(self):
        for key in probe.NATIVE_BOOLS:
            receipt, _ = self.exercise(native={**valid(), key: False})
            self.assertEqual(receipt["category"], "native_cleanup_or_identity_unverified")
        receipt, _ = self.exercise(native={**valid(), "admission_attempts": 0})
        self.assertEqual(receipt["category"], "native_observation_incomplete")

    def test_source_changes_and_cleanup_failure_cannot_pass(self):
        for changes, category in (({"source_change": True}, "source_commit_mismatch"),
                                  ({"dirty_after": True}, "source_checkout_dirty"),
                                  ({"cleanup_failure": True}, "driver_cleanup_failed")):
            receipt, _ = self.exercise(**changes)
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["category"], category)

    def test_primary_failure_survives_source_or_cleanup_failure(self):
        for changes in ({"source_change": True}, {"dirty_after": True}, {"cleanup_failure": True},
                        {"after_check_error": OSError("private error sentinel")}):
            receipt, calls = self.exercise(fail_stage="runtime", **changes)
            self.assertEqual(receipt["category"], "subprocess_timeout")
            self.assertEqual(sum(timeout == 60 for _, timeout, _ in calls), 1)
            self.assertNotIn("sentinel", json.dumps(receipt))

    def test_malformed_or_raw_failed_output_is_not_published(self):
        for data in (b"private-path and SID sentinel", b"x" * (probe.MAX_PROBE_BYTES + 1),
                     payload({**valid(), "raw": "sentinel"})):
            receipt, _ = self.exercise(native_data=data)
            self.assertEqual(receipt["category"], "invalid_output_file" if len(data) > probe.MAX_PROBE_BYTES
                             else "native_receipt_invalid")
            self.assertIsNone(receipt["probe"])
            self.assertEqual(receipt["source_check_after"], "matched")
            self.assertNotIn("sentinel", json.dumps(receipt))

    def test_fixed_budgets_and_caps(self):
        self.assertEqual((probe.COMPILE_TIMEOUT, probe.SELECTION_TIMEOUT, probe.RUNTIME_TIMEOUT,
                          probe.REAP_TIMEOUT), (120, 15, 60, 5))
        self.assertEqual((probe.MAX_COMPILE_BYTES, probe.MAX_LIST_BYTES, probe.MAX_PROBE_BYTES),
                         (4 * 1024 * 1024, 16 * 1024, 64 * 1024))


class CleanupTests(unittest.TestCase):
    def test_only_known_driver_logs_are_unlinked_and_unknown_entry_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve(strict=True)
            scratch = root / "driver"
            scratch.mkdir()
            (scratch / "compile.log").write_bytes(b"private")
            unknown = scratch / "unknown"
            unknown.mkdir()
            (unknown / "keep").write_bytes(b"keep")
            with self.assertRaises(OSError):
                probe.remove_driver_scratch(scratch)
            self.assertFalse((scratch / "compile.log").exists())
            self.assertEqual((unknown / "keep").read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
