#!/usr/bin/env python3
"""Deterministic receipt and driver tests; no Rust or agent is executed."""
from contextlib import redirect_stderr, redirect_stdout
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import scoped_search_acceptance as proof
from report_triage_diagnostics import CapturedFailure
from test_browse_acceptance import artifacts, encoded_artifacts


def execution(receipt=None, platform="linux"):
    value = proof.expected_receipt(platform) if receipt is None else receipt
    return ("running 1 test\ntest " + proof.TEST_NAME + " ... " + proof.PREFIX
            + json.dumps(value) + "\nok\n\ntest result: ok. 1 passed; 0 failed; "
            "0 ignored; 0 measured; 999 filtered out; finished in 0.01s\n").encode()


class ReceiptTests(unittest.TestCase):
    def test_exact_success_and_fixed_ten_operations(self):
        receipt = proof.expected_receipt()
        self.assertEqual(proof.validate_execution(execution()), receipt)
        self.assertEqual(len(receipt["operation_ledger"]), 10)

    def test_every_missing_extra_wrong_type_and_changed_value_rejected(self):
        receipt = proof.expected_receipt()
        invalid = [{**receipt, "extra": True}]
        for key, value in receipt.items():
            invalid.append({name: item for name, item in receipt.items() if name != key})
            invalid.append({**receipt, key: None})
            if type(value) is int:
                invalid.extend(({**receipt, key: value + 1}, {**receipt, key: float(value)}))
            elif type(value) is bool:
                invalid.extend(({**receipt, key: not value}, {**receipt, key: int(value)}))
        reordered = copy.deepcopy(receipt)
        reordered["operation_ledger"].reverse()
        invalid.append(reordered)
        invalid.extend(({**receipt, "os": "other"}, {**receipt, "source_version": "0.0.0"},
                        {**receipt, "cases": list(reversed(receipt["cases"]))},
                        {**receipt, "cases": receipt["cases"][:-1]}))
        for kind in ("hash", "scope", "mask", "limit_type"):
            changed = copy.deepcopy(receipt)
            if kind == "hash":
                changed["source_hashes"][0]["sha256"] = "0" * 64
            elif kind == "scope":
                changed["operation_ledger"][4]["request"]["scope"] = ""
            elif kind == "mask":
                changed["operation_ledger"][4]["request"]["masks"] = ["*"]
            else:
                changed["operation_ledger"][4]["request"]["limit"] = 500.0
            invalid.append(changed)
        for value in invalid:
            with self.subTest(value=value), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(execution(value))

    def test_exact_test_and_single_strict_json_receipt_required(self):
        good = execution()
        extra = (proof.PREFIX + json.dumps(proof.expected_receipt()) + "\n").encode()
        invalid = [good.replace(b"running 1 test", b"running 0 tests"),
                   good.replace(b"1 passed", b"0 passed"),
                   good.replace(proof.TEST_NAME.encode(), b"other_test"),
                   good.replace(proof.PREFIX.encode(), b"missing "), good + extra,
                   good.replace(b'{', b'{"duplicate": 1, "duplicate": 2,', 1),
                   good.replace(b'{', b'{"bad": NaN,', 1)]
        for data in invalid:
            with self.subTest(data=data), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(data)


class DriverTests(unittest.TestCase):
    def fixture(self, platform="linux", failure=None, invalid=None, mutate_agent=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            events = artifacts(root, platform)
            agent = root / "target/release" / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
            agent.parent.mkdir(parents=True)
            agent.write_bytes(b"owned mock agent, never executed")
            calls = []

            def runner(command, cwd, environment, timeout, limit):
                stage = "compile" if not calls else "runtime"
                calls.append((stage, command, timeout, limit, environment))
                self.assertEqual(cwd, root)
                self.assertEqual(environment["CEDAR_SCOPED_SEARCH_AGENT_BIN"], str(agent))
                self.assertEqual(environment["CARGO_TARGET_DIR"], str(root / "target"))
                if stage == failure:
                    raise CapturedFailure("subprocess_nonzero", b"bounded generated diagnostic\n",
                                          returncode=101, eof=True, cleanup="reaped")
                if stage == invalid:
                    return b"invalid output\n"
                if stage == "compile":
                    return encoded_artifacts(events)
                if mutate_agent:
                    agent.write_bytes(b"changed mock")
                return execution(platform=platform)

            native_platform = "win32" if platform == "windows" else "linux"
            output = io.StringIO()
            with mock.patch.object(proof.sys, "platform", native_platform), redirect_stdout(output), redirect_stderr(output):
                if failure or invalid or mutate_agent:
                    with self.assertRaises(proof.AcceptanceError):
                        proof.run(root, runner)
                else:
                    self.assertEqual(proof.run(root, runner), proof.expected_receipt(platform))
            self.assertEqual(calls[0][1], proof.compile_command())
            self.assertEqual(calls[0][2:4], (120, proof.MAX_COMPILE_BYTES))
            if len(calls) == 2:
                self.assertEqual(calls[1][1][1:], [proof.TEST_NAME, "--ignored", "--exact", "--test-threads=1", "--nocapture"])
                self.assertEqual(calls[1][2:4], (420, proof.MAX_RUNTIME_BYTES))
            return calls, output.getvalue()

    def test_both_platforms_preserve_exact_artifacts_commands_and_bounds(self):
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                calls, _ = self.fixture(platform)
                self.assertEqual(len(calls), 2)

    def test_each_stage_failure_stops_without_retry_or_cleanup_claim(self):
        for stage in ("compile", "runtime"):
            with self.subTest(stage=stage):
                calls, output = self.fixture(failure=stage)
                self.assertEqual(len(calls), 1 if stage == "compile" else 2)
                self.assertIn("bounded generated diagnostic", output)
                self.assertNotIn("acceptance passed", output)

    def test_invalid_stage_output_and_changed_release_agent_rejected(self):
        for stage in ("compile", "runtime"):
            with self.subTest(stage=stage):
                calls, _ = self.fixture(invalid=stage)
                self.assertEqual(len(calls), 1 if stage == "compile" else 2)
        self.fixture(mutate_agent=True)


if __name__ == "__main__":
    unittest.main()
