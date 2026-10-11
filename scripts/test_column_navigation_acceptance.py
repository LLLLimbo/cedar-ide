#!/usr/bin/env python3
"""Pure strict-contract and driver tests; never compile or execute Rust."""
from contextlib import redirect_stdout
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import column_navigation_acceptance as proof


DEFAULT_RECEIPT = object()


def payload(receipt=DEFAULT_RECEIPT, platform="linux"):
    receipt = proof.expected_receipt(platform) if receipt is DEFAULT_RECEIPT else receipt
    return ("running 1 test\n" + "test " + proof.TEST_NAME + " ... " + proof.PREFIX
            + json.dumps(receipt, ensure_ascii=False)
            + "\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "1900 filtered out; finished in 0.02s\n").encode()


def listing():
    return (proof.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n").encode()


def artifact(root, executable):
    return {"reason": "compiler-artifact", "manifest_path": str(root / "crates/app/Cargo.toml"),
            "target": {"name": "cedar_app", "kind": ["lib"],
                       "src_path": str(root / "crates/app/src/lib.rs")},
            "profile": {"test": True}, "executable": str(executable)}


def mutations(value):
    """Alter each scalar, key, order and nested entry independently."""
    for wrong in (None, True, False, 0, 1, 0.0, 1.0, "", [], {}):
        if not proof.typed_equal(value, wrong):
            yield wrong
    if type(value) is dict:
        yield {**value, "extra": True}
        for key in value:
            removed = dict(value)
            del removed[key]
            yield removed
            for changed in mutations(value[key]):
                yield {**value, key: changed}
    elif type(value) is list:
        yield value + [None]
        if value:
            yield value[:-1]
            yield value + [value[-1]]
        if len(value) > 1:
            yield list(reversed(value))
        for index, item in enumerate(value):
            for changed in mutations(item):
                yield value[:index] + [changed] + value[index + 1:]
    elif type(value) is str:
        yield value + "!"
        if value.upper() != value:
            yield value.upper()
    elif type(value) is int:
        yield value + 1
        yield float(value)
        yield str(value)


class ReceiptTests(unittest.TestCase):
    def test_exact_both_platform_receipt_with_lf_and_crlf(self):
        for platform in ("linux", "windows"):
            for ending in (b"\n", b"\r\n"):
                with self.subTest(platform=platform, ending=ending):
                    self.assertEqual(proof.validate_execution(
                        payload(platform=platform).replace(b"\n", ending), platform),
                        proof.expected_receipt(platform))

    def test_fixed_six_request_ledger_and_fourteen_cases(self):
        expected = proof.expected_receipt("linux")
        self.assertEqual(expected["cases"], [
            "unicode_crlf_coordinates", "unicode_lf_coordinates", "plain_line_compatibility",
            "invalid_coordinates_preserve_state", "cancel_preserves_full_selection",
            "same_location_preserves_history", "dirty_undo_redo",
            "stale_edit_before_release", "stale_edit_after_release",
            "stale_tab_before_release", "stale_navigation_after_release",
            "oversized_input_refused", "stale_reconnect", "history_after_reconnect",
        ])
        self.assertEqual(expected["operation_ledger"], [
            {"session": 1, "op": "Hello"}, {"session": 1, "op": "List", "path": ""},
            {"session": 1, "op": "Read", "path": "coordinates 😀/CRLF café.txt"},
            {"session": 1, "op": "Read", "path": "coordinates 😀/LF Ω.txt"},
            {"session": 2, "op": "Hello"}, {"session": 2, "op": "List", "path": ""},
        ])
        self.assertEqual((expected["requests"], expected["connections"], expected["agents_reaped"]),
                         (6, 2, 2))
        self.assertEqual((expected["shared_watchdog_seconds"], expected["total_budget_seconds"],
                          expected["request_admission_seconds"], expected["cleanup_timeout_seconds"]),
                         (60, 65, 30, 5))

    def test_every_value_key_type_and_nested_ledger_is_exact(self):
        expected = proof.expected_receipt("linux")
        for index, receipt in enumerate(mutations(expected)):
            with self.subTest(mutation=index), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(payload(receipt), "linux")

    def test_bad_json_duplicate_nested_key_nonfinite_and_prefix(self):
        good = payload()
        for bad in (
            good.replace(b'"schema": 1', b'"schema": 1, "schema": 1'),
            good.replace(b'"op": "Hello"', b'"op": "Hello", "op": "Hello"'),
            good.replace(b'"schema": 1', b'"schema": NaN'),
            good.replace(b'"schema": 1', b'"schema": Infinity'),
            good.replace(b'"schema": 1', b'"schema": -Infinity'),
            good.replace(b'"schema": 1', b'"schema": '),
            good.replace(proof.PREFIX.encode(), b""),
            good.replace(proof.PREFIX.encode(), b"unverified " + proof.PREFIX.encode()),
            good.replace(proof.PREFIX.encode(), b"Column_navigation_acceptance "),
            good.replace(proof.PREFIX.encode(), b"column_navigation_acceptance\t"),
            b"", b"\xff", good.decode(), b"x" * (proof.MAX_RUNTIME_BYTES + 1), good + good,
            payload(platform="windows"),
        ):
            with self.subTest(kind=type(bad)), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(bad, "linux")

    def test_receipt_requires_exact_one_successful_executed_test(self):
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

    def test_exact_selection_rejects_zero_extra_wrong_test_or_benchmark(self):
        proof.validate_test_listing(listing())
        proof.validate_test_listing(listing().replace(b"\n", b"\r\n"))
        for bad in (b"", b"\xff", listing() * 2, b"0 tests, 0 benchmarks\n",
                    listing().replace(proof.TEST_NAME.encode(), b"wrong_test"),
                    listing().replace(b": test", b": benchmark"),
                    listing().replace(b"1 test", b"2 tests"),
                    listing() + b"unexpected_test: test\n", b"x" * (proof.MAX_LIST_BYTES + 1)):
            with self.subTest(data=bad[:60]), self.assertRaises(proof.AcceptanceError):
                proof.validate_test_listing(bad)


class DriverTests(unittest.TestCase):
    def exercise(self, platform="linux", *, failure=None, selection=None, runtime=None,
                 missing_agent=False, invalid_artifact=False):
        with tempfile.TemporaryDirectory(prefix="cedar-column-driver-") as directory:
            root = Path(directory).resolve()
            deps, release = root / "target/debug/deps", root / "target/release"
            deps.mkdir(parents=True)
            release.mkdir()
            suffix = ".exe" if platform == "windows" else ""
            executable = deps / ("cedar_app-123abc" + suffix)
            executable.write_bytes(b"synthetic, never executed")
            agent = release / ("cedar-agent" + suffix)
            if not missing_agent:
                agent.write_bytes(b"synthetic, never executed")
            calls = []

            def execute(command, cwd, environment, timeout, limit):
                calls.append((command, timeout, limit))
                self.assertEqual(cwd, root)
                self.assertEqual(environment["CARGO_TARGET_DIR"], str(root / "target"))
                self.assertEqual(environment["CEDAR_COLUMN_NAVIGATION_AGENT_BIN"], str(agent))
                if command[0] == "cargo":
                    stage = "compile"
                    self.assertEqual(command, ["cargo", "test", "-p", "cedar-app", "--lib",
                                              "--locked", "--no-run", "--message-format=json"])
                    data = b"{}" if invalid_artifact else json.dumps(artifact(root, executable)).encode()
                elif "--list" in command:
                    stage = "selection"
                    self.assertEqual(command, [str(executable), proof.TEST_NAME, "--ignored", "--exact", "--list"])
                    data = listing() if selection is None else selection
                else:
                    stage = "runtime"
                    self.assertEqual(command, [str(executable), proof.TEST_NAME, "--ignored", "--exact",
                                               "--test-threads=1", "--nocapture"])
                    data = payload(platform=platform) if runtime is None else runtime
                if stage == failure:
                    raise proof.AcceptanceError("subprocess_timeout")
                return data

            capture = io.StringIO()
            with mock.patch.object(proof.sys, "platform", "win32" if platform == "windows" else platform), \
                    mock.patch.object(proof, "bounded_process", side_effect=execute), \
                    mock.patch.dict(proof.os.environ, {"CEDAR_COLUMN_NAVIGATION_AGENT_BIN": "wrong",
                                                      "CARGO_TARGET_DIR": "wrong"}), redirect_stdout(capture):
                receipt, error = None, None
                try:
                    receipt = proof.run(root)
                except proof.AcceptanceError as caught:
                    error = str(caught)
            return receipt, error, calls, capture.getvalue()

    def test_both_platforms_compile_select_execute_once_under_fixed_bounds(self):
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                receipt, error, calls, output = self.exercise(platform)
                self.assertIsNone(error)
                self.assertEqual(receipt, proof.expected_receipt(platform))
                self.assertEqual([timeout for _, timeout, _ in calls], [120, 15, 90])
                self.assertEqual([limit for _, _, limit in calls],
                                 [4 * 1024 * 1024, 16 * 1024, 256 * 1024])
                self.assertEqual(output.count(proof.PREFIX), 1)
                self.assertIn("14 cases, 6 setup requests, 2 agents reaped", output)

    def test_preparation_failure_never_starts_runtime(self):
        for changes, expected_calls in (({"missing_agent": True}, 0), ({"failure": "compile"}, 1),
                                        ({"invalid_artifact": True}, 1), ({"failure": "selection"}, 2),
                                        ({"selection": b"0 tests, 0 benchmarks\n"}, 2)):
            with self.subTest(changes=changes):
                receipt, error, calls, output = self.exercise(**changes)
                self.assertIsNone(receipt)
                self.assertIsNotNone(error)
                self.assertEqual(len(calls), expected_calls)
                self.assertEqual(output, "")

    def test_runtime_failure_receipt_mismatch_or_unverified_cleanup_never_retried(self):
        bad = copy.deepcopy(proof.expected_receipt("linux"))
        bad["agents_reaped"] = 1
        for changes in ({"failure": "runtime"}, {"runtime": b""}, {"runtime": payload() * 2},
                        {"runtime": payload(bad)}, {"runtime": payload(platform="windows")}):
            with self.subTest(changes=changes):
                receipt, error, calls, output = self.exercise(**changes)
                self.assertIsNone(receipt)
                self.assertIsNotNone(error)
                self.assertEqual(len(calls), 3)
                self.assertEqual(output, "")


if __name__ == "__main__":
    unittest.main()
