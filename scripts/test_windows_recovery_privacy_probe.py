#!/usr/bin/env python3
"""Cross-platform orchestration checks; no Rust build or native probe launch."""
from contextlib import redirect_stdout
import copy
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import windows_recovery_privacy_probe as probe


def listing():
    return (probe.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n").encode("utf-8")


def artifact(executable):
    return {"reason": "compiler-artifact", "target": {"name": probe.TEST_TARGET, "kind": ["test"]},
            "profile": {"test": True}, "executable": str(executable)}


def valid():
    return {
        "schema_version": 1, "probe": probe.TEST_NAME, "shipping_unchanged": True,
        "metadata_bytes_written": 0, "body_bytes_written": 0,
        "descriptor_query": "GetKernelObjectSecurity",
        "roots": [{
            "root": label, "outcome": "candidate_accepted", "stage": "complete", "category": "accepted",
            "objects_observed": 5, "descriptor_reads": 5, "candidate_accepts": 5,
            "candidate_rejections": 0, "ace_count": 15, "allow_ace_count": 15, "deny_ace_count": 0,
            "inherited_ace_count": 15, "inherit_only_ace_count": 0,
            **{key: True for key in probe.ROOT_BOOLS},
            "objects": [{"role": role, "outcome": "candidate_accepted", "category": "accepted",
                         "owner_matches": True, "ace_count": 3, "allow_ace_count": 3,
                         "deny_ace_count": 0, "inherited_ace_count": 3,
                         "inherit_only_ace_count": 0} for role in probe.OBJECT_ROLES],
        } for label in ("runner_temp", "local_app_data")],
    }


def payload(receipt):
    return ("running 1 test\ntest " + probe.TEST_NAME + " ... " + probe.PREFIX
            + json.dumps(receipt) + "\nok\n").encode("utf-8")


def reject(root, count):
    root.update(outcome="candidate_rejected", category="foreign_allow",
                candidate_rejections=count, candidate_accepts=5 - count)
    for item in root["objects"][:count]:
        item.update(outcome="candidate_rejected", category="foreign_allow")


class ReceiptTests(unittest.TestCase):
    def test_accepted_and_rejected_candidates_are_both_observations(self):
        for rejected in (0, 1, 5):
            receipt = valid()
            if rejected:
                reject(receipt["roots"][0], rejected)
            before = copy.deepcopy(receipt)
            decoded = probe.decode_probe(payload(receipt))
            probe.validate_observation(decoded)
            self.assertEqual(decoded, before)
            self.assertEqual(receipt, before)

    def test_explicit_native_error_is_preserved_but_never_an_observation(self):
        receipt = valid()
        receipt["roots"][0].update(outcome="error", stage="root", category="malformed_descriptor")
        self.assertEqual(probe.decode_probe(payload(receipt)), receipt)
        with self.assertRaisesRegex(ValueError, "native_probe_error"):
            probe.validate_observation(receipt)

    def test_descriptor_read_without_completed_assessment_remains_failure_evidence(self):
        receipt = valid()
        receipt["roots"][0].update(
            outcome="error", stage="root", category="malformed_descriptor",
            objects_observed=0, descriptor_reads=1, candidate_accepts=0,
            candidate_rejections=0, ace_count=0, allow_ace_count=0, deny_ace_count=0,
            inherited_ace_count=0, inherit_only_ace_count=0,
            rename_attempted=False, rename_completed=False, identity_stable=False,
            objects=[],
        )
        self.assertEqual(probe.decode_probe(payload(receipt)), receipt)
        with self.assertRaisesRegex(ValueError, "native_probe_error"):
            probe.validate_observation(receipt)

    def test_missing_extra_duplicate_and_malformed_evidence_rejected(self):
        data = payload(valid())
        malformed = [b"", b"running 0 tests\n", b"\xff", data + data,
                     probe.PREFIX.encode() + b"{invalid}", b"x" * (probe.MAX_PROBE_BYTES + 1),
                     data.replace(b'"schema_version": 1', b'"schema_version": 1, "schema_version": 1')]
        for value in malformed:
            with self.subTest(size=len(value)), self.assertRaisesRegex(ValueError, "native_receipt_invalid"):
                probe.decode_probe(value)
        for key in valid():
            receipt = valid()
            del receipt[key]
            with self.subTest(key=key), self.assertRaises(ValueError):
                probe.decode_probe(payload(receipt))
        for receipt in ({**valid(), "raw_path": "private sentinel"}, [], [valid()], None):
            with self.subTest(type=type(receipt)), self.assertRaises(ValueError):
                probe.decode_probe(payload(receipt))

    def test_each_field_has_strict_type_and_bound(self):
        for key, upper in probe.ROOT_COUNTS.items():
            for replacement in (-1, upper + 1, True, 1.0, "1", None, float("nan")):
                receipt = valid()
                receipt["roots"][0][key] = replacement
                with self.subTest(key=key, value=replacement), self.assertRaises(ValueError):
                    probe.decode_probe(payload(receipt))
        for key in probe.ROOT_BOOLS:
            for replacement in (0, 1, "true", None):
                receipt = valid()
                receipt["roots"][0][key] = replacement
                with self.subTest(key=key), self.assertRaises(ValueError):
                    probe.decode_probe(payload(receipt))
        for key, replacement in (("root", "private path"), ("outcome", "success"),
                                 ("category", "private SID"), ("stage", "private error"),
                                 ("extra", "private descriptor")):
            receipt = valid()
            receipt["roots"][0][key] = replacement
            with self.subTest(key=key), self.assertRaises(ValueError):
                probe.decode_probe(payload(receipt))
        for key in ("schema_version", "metadata_bytes_written", "body_bytes_written"):
            for replacement in (True, -1, "0"):
                receipt = valid()
                receipt[key] = replacement
                with self.subTest(key=key), self.assertRaises(ValueError):
                    probe.decode_probe(payload(receipt))

    def test_false_witness_partial_counts_and_inconsistent_outcome_do_not_pass(self):
        changes = [{key: False} for key in probe.ROOT_BOOLS]
        changes += [{"objects_observed": 4, "descriptor_reads": 4, "candidate_accepts": 4},
                    {"stage": "renamed"}, {"category": "foreign_allow"}]
        for fields in changes:
            receipt = valid()
            receipt["roots"][0].update(fields)
            if "objects_observed" in fields:
                root = receipt["roots"][0]
                root["objects"].pop()
                for key in probe.ACE_COUNTS:
                    root[key] = sum(item[key] for item in root["objects"])
            decoded = probe.decode_probe(payload(receipt))
            with self.subTest(fields=fields), self.assertRaises(ValueError):
                probe.validate_observation(decoded)
        receipt = valid()
        receipt["roots"].reverse()
        with self.assertRaises(ValueError):
            probe.decode_probe(payload(receipt))

    def test_per_object_roles_must_be_unique_and_in_exact_prefix_order(self):
        for change in ("duplicate", "reverse", "unknown", "missing", "six"):
            receipt = valid()
            objects = receipt["roots"][0]["objects"]
            if change == "duplicate":
                objects[1]["role"] = "root"
            elif change == "reverse":
                objects.reverse()
            elif change == "unknown":
                objects[1]["role"] = "private path"
            elif change == "missing":
                objects.pop(0)
            else:
                objects.append(copy.deepcopy(objects[-1]))
            with self.subTest(change=change), self.assertRaises(ValueError):
                probe.decode_probe(payload(receipt))

    def test_per_object_fields_types_categories_and_sums_are_strict(self):
        changes = [{key: value} for key in probe.ACE_COUNTS for value in (-1, 129, True, 1.0, "1")]
        changes += [{"extra": "private descriptor"}, {"owner_matches": 1},
                    {"owner_matches": False}, {"outcome": "error"}, {"category": "private SID"},
                    {"ace_count": 4}, {"allow_ace_count": 2}, {"inherited_ace_count": 2},
                    {"deny_ace_count": 1}, {"inherit_only_ace_count": 1}]
        for fields in changes:
            receipt = valid()
            receipt["roots"][0]["objects"][0].update(fields)
            with self.subTest(fields=fields), self.assertRaises(ValueError):
                probe.decode_probe(payload(receipt))
        for key in valid()["roots"][0]["objects"][0]:
            receipt = valid()
            del receipt["roots"][0]["objects"][0][key]
            with self.subTest(missing=key), self.assertRaises(ValueError):
                probe.decode_probe(payload(receipt))

    def test_first_rejected_object_must_match_root_summary(self):
        receipt = valid()
        reject(receipt["roots"][0], 2)
        receipt["roots"][0]["objects"][0].update(category="owner_mismatch", owner_matches=False)
        with self.assertRaises(ValueError):
            probe.decode_probe(payload(receipt))
        receipt["roots"][0]["category"] = "owner_mismatch"
        self.assertEqual(probe.decode_probe(payload(receipt)), receipt)
        probe.validate_observation(receipt)

    def test_unapproved_descriptor_api_identity_is_rejected(self):
        receipt = valid()
        for value in ("GetSecurityInfo", None, True, "private API details"):
            receipt["descriptor_query"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                probe.decode_probe(payload(receipt))


class SelectionTests(unittest.TestCase):
    def test_exact_ignored_test_and_crlf_listing(self):
        probe.validate_test_listing(listing())
        probe.validate_test_listing(listing().replace(b"\n", b"\r\n"))
        self.assertEqual(probe.test_compile_command(), [
            "cargo", "test", "-p", "cedar-recovery", "--test", "windows_privacy_probe",
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
            executable = deps / "windows_privacy_probe-123abc.exe"
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
                         deps / "windows_privacy_probe-123abc.dll"):
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
                 dirty=False, cleanup_failure=False):
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
            executable = deps / "windows_privacy_probe-123abc.exe"
            executable.write_bytes(b"synthetic binary, never executed")
            calls = []
            seen_heads = 0

            def run(command, cwd, environment, log, timeout, limit):
                nonlocal seen_heads
                calls.append((command, timeout, limit))
                if command[:2] == ["git", "rev-parse"]:
                    seen_heads += 1
                    data = (("b" if source_change and seen_heads == 2 else "a") * 40 + "\n").encode()
                elif command[:2] == ["git", "status"]:
                    data = b" M private-path-never-published\n" if dirty else b""
                elif command[0] == "cargo":
                    self.assertEqual(environment["CARGO_TARGET_DIR"], str(root / "target"))
                    data = json.dumps(artifact(executable)).encode() + b"\n"
                elif "--list" in command:
                    data = listing() if selection is None else selection
                else:
                    self.assertEqual(command, [str(executable), "--ignored", "--exact", probe.TEST_NAME,
                                               "--nocapture", "--test-threads=1"])
                    data = payload(native)
                log.write_bytes(data)
                stage = "compile" if command[0] == "cargo" else (
                    "selection" if "--list" in command else "runtime" if command[0] == str(executable) else "source")
                if stage == fail_stage:
                    raise probe.ProbeError("subprocess_timeout")

            capture = io.StringIO()
            remove = probe.shutil.rmtree

            def cleanup(path):
                if cleanup_failure:
                    raise OSError("private cleanup message")
                remove(path)

            with mock.patch.object(probe.sys, "platform", "win32"), \
                    mock.patch.object(probe, "bounded_process", side_effect=run), \
                    mock.patch.object(probe.shutil, "rmtree", side_effect=cleanup), redirect_stdout(capture):
                result = probe.run(root, scratch_root, "a" * 40)
            receipt = json.loads((scratch_root / probe.EVIDENCE_NAME).read_bytes())
            self.assertEqual(result, receipt)
            self.assertEqual(json.loads(capture.getvalue()), receipt)
            if not cleanup_failure:
                self.assertEqual(set(path.name for path in scratch_root.iterdir()), {probe.EVIDENCE_NAME})
            self.assertEqual(receipt["driver_scratch_removed"], not cleanup_failure)
            self.assertNotIn(str(owned), capture.getvalue())
            return receipt, calls

    def test_one_runtime_after_compile_and_exact_selection(self):
        receipt, calls = self.exercise()
        self.assertEqual(receipt["status"], "observed")
        self.assertTrue(receipt["source_verified_before"] and receipt["source_verified_after"])
        self.assertTrue(receipt["exact_one_test_selected"])
        self.assertEqual(receipt["runtime_invocations"], 1)
        self.assertEqual([timeout for _, timeout, _ in calls], [10, 10, 120, 15, 60, 10, 10])

    def test_rejected_candidate_is_a_completed_observation(self):
        native = valid()
        reject(native["roots"][0], 5)
        receipt, _ = self.exercise(native=native)
        self.assertEqual(receipt["status"], "observed")
        self.assertEqual(receipt["probe"], native)

    def test_preparation_or_selection_failure_never_starts_runtime(self):
        for fail_stage in ("compile", "selection"):
            with self.subTest(stage=fail_stage):
                receipt, _ = self.exercise(fail_stage=fail_stage)
                self.assertEqual(receipt["status"], "failed")
                self.assertEqual(receipt["stage"], fail_stage)
                self.assertEqual(receipt["runtime_invocations"], 0)
        receipt, _ = self.exercise(selection=b"0 tests, 0 benchmarks\n")
        self.assertEqual(receipt["category"], "exact_one_test_missing")
        self.assertEqual(receipt["runtime_invocations"], 0)

    def test_failed_runtime_is_not_retried_even_with_complete_receipt(self):
        receipt, calls = self.exercise(fail_stage="runtime")
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["category"], "subprocess_timeout")
        self.assertEqual(receipt["probe"], valid())
        self.assertEqual(sum(timeout == 60 for _, timeout, _ in calls), 1)

    def test_changed_source_cannot_observe_success(self):
        receipt, _ = self.exercise(source_change=True)
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["category"], "source_commit_mismatch")
        self.assertFalse(receipt["source_verified_after"])

    def test_dirty_source_blocks_compilation_and_native_runtime(self):
        receipt, calls = self.exercise(dirty=True)
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["category"], "source_checkout_dirty")
        self.assertEqual(receipt["runtime_invocations"], 0)
        self.assertEqual(len(calls), 2)
        self.assertNotIn("private-path", json.dumps(receipt))

    def test_driver_cleanup_failure_is_explicit_and_retains_receipt(self):
        receipt, _ = self.exercise(cleanup_failure=True)
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["stage"], "cleanup")
        self.assertEqual(receipt["category"], "driver_cleanup_failed")
        self.assertNotIn("private cleanup", json.dumps(receipt))

    def test_native_api_error_and_cleanup_failure_are_explicit_failed_receipts(self):
        native = valid()
        native["roots"][0].update(outcome="error", category="malformed_descriptor", stage="root")
        receipt, _ = self.exercise(native=native)
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["category"], "native_probe_error")
        self.assertEqual(receipt["probe"], native)
        native = valid()
        native["roots"][0]["cleanup_complete"] = False
        receipt, _ = self.exercise(native=native)
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["category"], "native_cleanup_or_identity_unverified")

    def test_malformed_native_text_is_not_published(self):
        native = {**valid(), "raw_private": "NEVER_PUBLISH_THIS_SENTINEL"}
        receipt, _ = self.exercise(native=native)
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["category"], "native_receipt_invalid")
        self.assertIsNone(receipt["probe"])
        self.assertNotIn("NEVER_PUBLISH_THIS_SENTINEL", json.dumps(receipt))


if __name__ == "__main__":
    unittest.main()
