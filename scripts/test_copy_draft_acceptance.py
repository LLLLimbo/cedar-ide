#!/usr/bin/env python3
"""Pure adversarial contract/supervisor tests; never compile or run Rust."""
from contextlib import redirect_stdout
import copy
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import copy_draft_acceptance as proof


def payload(receipts=None, platform="linux"):
    if receipts is None:
        receipts = proof.expected_receipts(platform)
    return ("running 1 test\n"
            "test " + proof.TEST_NAME + " ... "
            + "\n".join(proof.PREFIX + json.dumps(receipt, ensure_ascii=False) for receipt in receipts)
            + "\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "1899 filtered out; finished in 0.02s\n").encode("utf-8")


def listing():
    return (proof.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n").encode()


def artifact(root, executable):
    return {"reason": "compiler-artifact", "manifest_path": str(root / "crates/app/Cargo.toml"),
            "target": {"name": "cedar_app", "kind": ["lib"],
                       "src_path": str(root / "crates/app/src/lib.rs")},
            "profile": {"test": True}, "executable": str(executable)}


class ReceiptTests(unittest.TestCase):
    def reject(self, receipts):
        with self.assertRaises(proof.AcceptanceError):
            proof.validate_execution(payload(receipts), "linux")

    def test_exact_both_platform_receipts_with_lf_and_crlf(self):
        for platform in ("linux", "windows"):
            for ending in (b"\n", b"\r\n"):
                with self.subTest(platform=platform, ending=ending):
                    data = payload(platform=platform).replace(b"\n", ending)
                    self.assertEqual(proof.validate_execution(data, platform),
                                     proof.expected_receipts(platform))

    def test_fixture_contract_independent_counts_and_ledger(self):
        receipts = proof.expected_receipts("linux")
        self.assertEqual([r["test"] for r in receipts], [
            "normal_dirty_explicit_save", "normal_preexisting_destination",
            "normal_presave_destination_race", "normal_source_changed_reject",
            "normal_reconnect_reject", "controlled_unknown_source_copy",
            "controlled_copy_save_unknown"])
        self.assertEqual([r["requests"] for r in receipts], [5, 4, 4, 3, 5, 8, 6])
        self.assertEqual([r["list"] for r in receipts], [2, 1, 1, 1, 2, 3, 2])
        for key, count in {"requests": 35, "hello": 10, "list": 12, "read": 7, "write": 6,
                           "connections": 10, "reaped": 10, "successful_ack_refresh_lists": 2,
                           "recovery_workers_started": 7, "recovery_workers_stopped": 7}.items():
            self.assertEqual(sum(r[key] for r in receipts), count)
        draft_digest = hashlib.sha256(
            "unsaved draft β\nsecond café\nthird line\n".encode()).hexdigest()
        base_digest = hashlib.sha256("original α\nsecond café\nthird line\n".encode()).hexdigest()
        self.assertEqual(receipts[5]["operation_ledger"], [
            {"op": "Hello"}, {"op": "List", "path": ""}, {"op": "Read", "path": "draft.txt"},
            {"op": "Write", "path": "draft.txt", "expected_revision": base_digest,
             "text_sha256": draft_digest},
            {"op": "Hello"}, {"op": "List", "path": ""},
            {"op": "Write", "path": "copy café.txt", "expected_revision": None,
             "text_sha256": draft_digest}, {"op": "List", "path": ""}])
        self.assertEqual(receipts[5]["controlled_operation_ledger"], ["Hello", "List", "Read", "Write"])
        self.assertEqual(receipts[6]["controlled_operation_ledger"], ["Hello", "List", "Write"])

    def test_every_field_is_required_and_no_extra_fields_allowed(self):
        for index, receipt in enumerate(proof.expected_receipts("linux")):
            for key in receipt:
                receipts = proof.expected_receipts("linux")
                del receipts[index][key]
                with self.subTest(case=index, missing=key):
                    self.reject(receipts)
            receipts = proof.expected_receipts("linux")
            receipts[index]["extra"] = True
            self.reject(receipts)

    def test_every_scalar_has_exact_type_and_value(self):
        for index, receipt in enumerate(proof.expected_receipts("linux")):
            for key, value in receipt.items():
                if type(value) is list:
                    continue
                bad_values = ([], {}, None, "", 0, 1, 0.0, 1.0, True, False)
                if type(value) is str:
                    bad_values += (value + "!", value.upper())
                elif type(value) is int:
                    bad_values += (value + 1, float(value), str(value))
                for bad in bad_values:
                    if proof.typed_equal(bad, value):
                        continue
                    receipts = proof.expected_receipts("linux")
                    receipts[index][key] = bad
                    with self.subTest(case=index, key=key, bad=bad):
                        self.reject(receipts)

    def test_absent_duplicate_extra_wrong_order_or_wrong_case_receipts_rejected(self):
        good = proof.expected_receipts("linux")
        for bad in ([], good[:-1], good[1:], good + [good[-1]], [good[0]] * 7,
                    list(reversed(good)), [good[1], good[0], *good[2:]]):
            with self.subTest(length=len(bad)):
                self.reject(bad)
        for index in range(7):
            bad = copy.deepcopy(good)
            bad[index]["test"] = "normal_Dirty_explicit_save"
            self.reject(bad)

    def test_nested_ledgers_require_all_and_only_exact_operations(self):
        good = proof.expected_receipts("linux")
        for index, receipt in enumerate(good):
            for operation_index, entry in enumerate(receipt["operation_ledger"]):
                replacements = [None, [], "Hello", {}, {**entry, "extra": False}]
                for key, value in entry.items():
                    removed = dict(entry)
                    del removed[key]
                    replacements.append(removed)
                    for bad in (None, True, False, 0, 1, "", "../other", "0" * 64,
                                [], {}, "false", value.upper() if type(value) is str else "absent"):
                        if not proof.typed_equal(bad, value):
                            replacements.append({**entry, key: bad})
                for replacement in replacements:
                    bad = copy.deepcopy(good)
                    bad[index]["operation_ledger"][operation_index] = replacement
                    with self.subTest(case=index, operation=operation_index, replacement=replacement):
                        self.reject(bad)
            for field in ("operation_ledger", "controlled_operation_ledger"):
                ledger = receipt[field]
                for replacement in (None, {}, "", False, ledger + [{"op": "Run"}],
                                    ledger[:-1] if ledger else ["Hello"],
                                    list(reversed(ledger)) if len(ledger) > 1 else ["Read"]):
                    bad = copy.deepcopy(good)
                    bad[index][field] = replacement
                    self.reject(bad)

    def test_duplicate_json_keys_nonfinite_and_malformed_json_rejected(self):
        good = payload()
        for bad in (
            good.replace(b'"schema": 1', b'"schema": 1, "schema": 1', 1),
            good.replace(b'"op": "Hello"', b'"op": "Hello", "op": "Hello"', 1),
            good.replace(b'"schema": 1', b'"schema": NaN', 1),
            good.replace(b'"schema": 1', b'"schema": Infinity', 1),
            good.replace(b'"schema": 1', b'"schema": -Infinity', 1),
            good.replace(b'"schema": 1', b'"schema": ', 1),
        ):
            with self.subTest(prefix=bad[:100]), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(bad, "linux")

    def test_raw_json_unmarked_malformed_or_wrong_prefix_not_receipts(self):
        good = payload()
        for bad in (b"", b"\xff", good.decode(), b"x" * (proof.MAX_RUNTIME_BYTES + 1),
                    good.replace(proof.PREFIX.encode(), b""),
                    good.replace(proof.PREFIX.encode(), b"Copy_draft_acceptance ", 1),
                    good.replace(proof.PREFIX.encode(), b"unverified " + proof.PREFIX.encode(), 1),
                    good.replace(proof.PREFIX.encode(), b"copy_draft_acceptance\t", 1),
                    good.replace(proof.PREFIX.encode(), proof.PREFIX.encode() + b"[] ", 1),
                    good + good):
            with self.subTest(kind=type(bad)), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(bad, "linux")

    def test_receipts_need_exactly_one_successful_executed_test(self):
        good = payload()
        for old, new in ((b"running 1 test", b"running 0 tests"),
                         (b"running 1 test", b"running 2 tests"),
                         (b"running 1 test", b""), (b"1 passed", b"0 passed"),
                         (b"0 failed", b"1 failed"), (b"0 ignored", b"1 ignored"),
                         (b"0 measured", b"1 measured"), (b"ok. 1 passed", b"FAILED. 1 passed")):
            with self.subTest(old=old), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(good.replace(old, new), "linux")
        for extra in (b"running 1 test\n", good.splitlines()[-1] + b"\n"):
            with self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(good + extra, "linux")

    def test_booleans_are_not_integers_even_recursively(self):
        for actual, expected in ((True, 1), (False, 0), ([True], [1]),
                                 ({"nested": [0]}, {"nested": [False]}),
                                 (1.0, 1), ({"a": 1, "b": 2}, {"a": 1})):
            self.assertFalse(proof.typed_equal(actual, expected))


class SelectionTests(unittest.TestCase):
    def test_compile_and_exact_ignored_listing_are_fixed(self):
        self.assertEqual(proof.test_compile_command(), ["cargo", "test", "-p", "cedar-app", "--lib",
                         "--locked", "--no-run", "--message-format=json"])
        proof.validate_test_listing(listing())
        proof.validate_test_listing(listing().replace(b"\n", b"\r\n"))

    def test_absent_extra_duplicate_wrong_or_malformed_listing_rejected(self):
        for data in (b"", b"\xff", b"x" * (proof.MAX_LIST_BYTES + 1),
                     b"0 tests, 0 benchmarks\n", listing() * 2,
                     listing().replace(proof.TEST_NAME.encode(), b"wrong_test"),
                     listing().replace(b": test", b": benchmark"),
                     listing().replace(b"1 test", b"2 tests"),
                     listing() + b"additional_test: test\n", listing() + b"unexpected output\n"):
            with self.subTest(data=data[:60]), self.assertRaises(proof.AcceptanceError):
                proof.validate_test_listing(data)

    def test_exact_cargo_library_artifact_required_on_both_platforms(self):
        with tempfile.TemporaryDirectory(prefix="cedar-copy-artifact-") as directory:
            root = Path(directory).resolve()
            deps = root / "target/debug/deps"
            deps.mkdir(parents=True)
            for platform, suffix in (("linux", ""), ("windows", ".exe")):
                executable = deps / ("cedar_app-123abc" + suffix)
                executable.write_bytes(b"synthetic: never executed")
                event = artifact(root, executable)
                self.assertEqual(proof.compiled_test(json.dumps(event).encode(), root, platform), executable)
                dependency = {"reason": "compiler-artifact", "target": {"name": "cedar_client"}}
                data = b"Cargo progress\n" + json.dumps(dependency).encode() + b"\n" + json.dumps(event).encode()
                self.assertEqual(proof.compiled_test(data, root, platform), executable)
                invalid_events = [[], [event, event], [{**event, "executable": None}],
                                  [{**event, "profile": {"test": 1}}],
                                  [{**event, "profile": {"test": False}}],
                                  [{**event, "profile": []}], [{**event, "target": []}],
                                  [{**event, "manifest_path": str(root / "other/Cargo.toml")}],
                                  [{**event, "target": {**event["target"], "src_path": "elsewhere.rs"}}],
                                  [{**event, "target": {**event["target"], "kind": ["bin"]}}],
                                  [{**event, "target": {**event["target"], "name": "other"}}]]
                for events in invalid_events:
                    data = b"\n".join(json.dumps(value).encode() for value in events)
                    with self.subTest(platform=platform, events=events), self.assertRaises(proof.AcceptanceError):
                        proof.compiled_test(data, root, platform)
                for path in (root / executable.name, deps / ("other-123abc" + suffix),
                             deps / "cedar_app-123abc.dll", deps / ("cedar_app-nothex" + suffix),
                             Path("target/debug/deps") / executable.name):
                    with self.subTest(path=path), self.assertRaises(proof.AcceptanceError):
                        proof.compiled_test(json.dumps(artifact(root, path)).encode(), root, platform)
                executable.unlink()
                with self.assertRaises(proof.AcceptanceError):
                    proof.compiled_test(json.dumps(event).encode(), root, platform)

    def test_compile_output_encoding_limit_and_duplicate_fields(self):
        with tempfile.TemporaryDirectory(prefix="cedar-copy-artifact-") as directory:
            root = Path(directory).resolve()
            for data in (b"\xff", b"x" * (proof.MAX_COMPILE_BYTES + 1),
                         b'{"reason":"compiler-artifact","reason":"compiler-artifact"}',
                         b'{"reason":NaN}', b"{bad json}", b"{}"):
                with self.subTest(data=data[:50]), self.assertRaises(proof.AcceptanceError):
                    proof.compiled_test(data, root, "linux")


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

        with mock.patch.object(proof.subprocess, "Popen", return_value=process) as launch, \
                mock.patch.object(proof.os, "set_blocking") as blocking, \
                mock.patch.object(proof.os, "read", side_effect=read), \
                mock.patch.object(proof.time, "monotonic", side_effect=now), \
                mock.patch.object(proof.time, "sleep"):
            output, error = None, None
            try:
                output = proof.bounded_process(["synthetic child, never launched"], Path("."), {}, timeout, limit)
            except proof.AcceptanceError as caught:
                error = str(caught)
        self.assertEqual(launch.call_count, 1)
        blocking.assert_called_once_with(42, False)
        process.stdout.close.assert_called_once()
        process.wait.assert_called_once_with(timeout=5)
        return output, error, process

    def test_complete_output_drained_and_owned_child_reaped(self):
        output, error, process = self.exercise([BlockingIOError(), b"one\n", b"two\n", b""])
        self.assertEqual(output, b"one\ntwo\n")
        self.assertIsNone(error)
        process.kill.assert_not_called()

    def test_exact_output_limit_accepted_and_one_byte_more_rejected(self):
        output, error, _ = self.exercise([b"12345678", b""], limit=8)
        self.assertEqual(output, b"12345678")
        self.assertIsNone(error)
        output, error, process = self.exercise([b"12345678", b"x"], limit=8, returncode=None)
        self.assertIsNone(output)
        self.assertEqual(error, "subprocess_output_limit")
        process.kill.assert_called_once()

    def test_deadline_covers_process_and_pipe_including_after_exit(self):
        for returncode in (None, 0):
            with self.subTest(returncode=returncode):
                output, error, process = self.exercise([], returncode=returncode, timeout=0.1)
                self.assertIsNone(output)
                self.assertEqual(error, "subprocess_timeout")
                self.assertEqual(process.kill.call_count, int(returncode is None))

    def test_nonzero_or_failed_reap_cannot_return_output(self):
        for changes, expected in (({"returncode": 7}, "subprocess_nonzero"),
                                  ({"cleanup_error": subprocess.TimeoutExpired("synthetic", 5)},
                                   "child_cleanup_unverified")):
            output, error, _ = self.exercise([b""], **changes)
            self.assertIsNone(output)
            self.assertEqual(error, expected)


class DriverTests(unittest.TestCase):
    def exercise(self, platform="linux", *, stage_failure=None, selection=None, runtime=None,
                 missing_agent=None, invalid_artifact=False):
        with tempfile.TemporaryDirectory(prefix="cedar-copy-driver-") as directory:
            root = Path(directory).resolve()
            deps = root / "target/debug/deps"
            release = root / "target/release"
            deps.mkdir(parents=True)
            release.mkdir()
            suffix = ".exe" if platform == "windows" else ""
            executable = deps / ("cedar_app-123abc" + suffix)
            executable.write_bytes(b"synthetic, never executed")
            for name in ("cedar-agent", "cedar-agent-interrupted-save-validation"):
                if name != missing_agent:
                    (release / (name + suffix)).write_bytes(b"synthetic, never executed")
            calls = []

            def execute(command, cwd, environment, timeout, limit):
                calls.append((command, timeout, limit))
                self.assertEqual(cwd, root)
                self.assertEqual(environment["CARGO_TARGET_DIR"], str(root / "target"))
                self.assertEqual(environment["CEDAR_COPY_DRAFT_AGENT_BIN"], str(release / ("cedar-agent" + suffix)))
                self.assertEqual(environment["CEDAR_INTERRUPTED_SAVE_AGENT_BIN"],
                                 str(release / ("cedar-agent-interrupted-save-validation" + suffix)))
                if command[0] == "cargo":
                    stage = "compile"
                    self.assertEqual(command, proof.test_compile_command())
                    data = json.dumps(artifact(root, executable)).encode()
                    if invalid_artifact:
                        data = b"{}"
                elif "--list" in command:
                    stage = "selection"
                    self.assertEqual(command, [str(executable), proof.TEST_NAME, "--ignored", "--exact", "--list"])
                    data = listing() if selection is None else selection
                else:
                    stage = "runtime"
                    self.assertEqual(command, [str(executable), proof.TEST_NAME, "--ignored", "--exact",
                                               "--test-threads=1", "--nocapture"])
                    data = payload(platform=platform) if runtime is None else runtime
                if stage == stage_failure:
                    raise proof.AcceptanceError("subprocess_timeout")
                return data

            capture = io.StringIO()
            with mock.patch.object(proof.sys, "platform", "win32" if platform == "windows" else platform), \
                    mock.patch.object(proof, "bounded_process", side_effect=execute), \
                    mock.patch.dict(proof.os.environ, {"CEDAR_COPY_DRAFT_AGENT_BIN": "wrong",
                                                      "CEDAR_INTERRUPTED_SAVE_AGENT_BIN": "wrong",
                                                      "CARGO_TARGET_DIR": "wrong"}), redirect_stdout(capture):
                receipts, error = None, None
                try:
                    receipts = proof.run(root)
                except proof.AcceptanceError as caught:
                    error = str(caught)
            return receipts, error, calls, capture.getvalue()

    def test_one_runtime_after_compile_and_exact_selection_both_platforms(self):
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                receipts, error, calls, output = self.exercise(platform)
                self.assertIsNone(error)
                self.assertEqual(receipts, proof.expected_receipts(platform))
                self.assertEqual([timeout for _, timeout, _ in calls], [120, 15, 180])
                self.assertEqual([limit for _, _, limit in calls],
                                 [4 * 1024 * 1024, 16 * 1024, 256 * 1024])
                self.assertEqual(output.count(proof.PREFIX), 7)
                self.assertIn("35 requests, 10 agents reaped", output)

    def test_preparation_failure_never_starts_runtime(self):
        for changes, expected_calls in (({"missing_agent": "cedar-agent"}, 0),
                                        ({"missing_agent": "cedar-agent-interrupted-save-validation"}, 0),
                                        ({"stage_failure": "compile"}, 1),
                                        ({"invalid_artifact": True}, 1),
                                        ({"stage_failure": "selection"}, 2),
                                        ({"selection": b"0 tests, 0 benchmarks\n"}, 2)):
            with self.subTest(changes=changes):
                receipts, error, calls, output = self.exercise(**changes)
                self.assertIsNone(receipts)
                self.assertIsNotNone(error)
                self.assertEqual(len(calls), expected_calls)
                self.assertEqual(output, "")

    def test_runtime_failure_or_invalid_receipts_never_retried_or_printed(self):
        for changes in ({"stage_failure": "runtime"}, {"runtime": b""},
                        {"runtime": payload() * 2}, {"runtime": payload(platform="windows")}):
            with self.subTest(changes=changes):
                receipts, error, calls, output = self.exercise(**changes)
                self.assertIsNone(receipts)
                self.assertIsNotNone(error)
                self.assertEqual(len(calls), 3)
                self.assertEqual(output, "")


if __name__ == "__main__":
    unittest.main()
