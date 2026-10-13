#!/usr/bin/env python3
"""Strict pure receipt/selection tests. Never launches a process or compiles Rust."""
import copy
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import file_structure_acceptance as proof
from test_column_navigation_acceptance import mutations


DEFAULT_RECEIPT = object()


def payload(receipt=DEFAULT_RECEIPT):
    if receipt is DEFAULT_RECEIPT:
        receipt = proof.expected_receipt()
    return ("running 1 test\n" + "test " + proof.TEST_NAME + " ... " + proof.PREFIX
            + json.dumps(receipt, ensure_ascii=True)
            + "\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "1900 filtered out; finished in 0.02s\n").encode()


class ContractTests(unittest.TestCase):
    def test_valid_portable_stdout(self):
        for ending in (b"\n", b"\r\n"):
            self.assertEqual(proof.validate_execution(payload().replace(b"\n", ending)),
                             proof.expected_receipt())

    def test_exact_fixed_ledger_and_full_client_bounds(self):
        receipt = proof.expected_receipt()
        self.assertEqual([entry["op"] for entry in receipt["operation_ledger"]], [
            "Hello", "List", "Read", "LanguageStart", "LanguageOpen",
            "LanguageDocumentSymbols", "LanguageChange", "LanguageDocumentSymbols",
            "LanguageDocumentSymbols", "LanguageClose", "LanguageStop"])
        self.assertEqual(receipt["requests"], len(receipt["operation_ledger"]))
        self.assertEqual(receipt["active_seconds"], 75 + 8 * 30 + 15)
        self.assertEqual(receipt["cleanup_reserve_seconds"], 30 + 30 + 4)
        self.assertEqual(receipt["total_seconds"], 330 + 64)
        self.assertGreater(proof.RUNTIME_TIMEOUT, receipt["total_seconds"])
        self.assertLessEqual(proof.COMPILE_TIMEOUT + proof.SELECTION_TIMEOUT
                             + proof.RUNTIME_TIMEOUT + 3 * 5, 600)

    def test_every_key_value_type_and_nested_ledger_required(self):
        for index, changed in enumerate(mutations(proof.expected_receipt())):
            with self.subTest(index=index), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(payload(changed))

    def test_unmeasured_platforms_cannot_be_promoted(self):
        for key in ("native_jdt", "windows_provider", "native_gui"):
            changed = copy.deepcopy(proof.expected_receipt())
            changed[key] = "passed"
            with self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(payload(changed))
        with mock.patch.object(proof.sys, "platform", "win32"), \
                mock.patch.object(proof, "bounded_process") as process, \
                redirect_stdout(io.StringIO()), \
                self.assertRaises(proof.AcceptanceError):
            proof.run(".")
        process.assert_not_called()

    def test_bad_json_duplicate_fields_wrong_prefix_failed_and_repeated_receipts(self):
        good = payload()
        for bad in (
            b"", b"\xff", good + good, good.decode(), b"x" * (proof.MAX_RUNTIME_BYTES + 1),
            good.replace(b'"schema": 1', b'"schema": 1, "schema": 1'),
            good.replace(b'"op": "Hello"', b'"op": "Hello", "op": "Hello"'),
            good.replace(b'"schema": 1', b'"schema": NaN'),
            good.replace(b'"schema": 1', b'"schema": Infinity'),
            good.replace(b'"success": true', b'"success": false'),
            good.replace(b"1 passed; 0 failed", b"0 passed; 1 failed"),
            good.replace(b"running 1 test", b"running 0 tests"),
            good.replace(b"0 ignored", b"1 ignored"),
            good.replace(proof.PREFIX.encode(), b"unverified " + proof.PREFIX.encode()),
            good.replace(proof.PREFIX.encode(), b"file_structure_acceptance\t"),
            good.replace(b'"os": "linux"', b'"os": "windows"'),
            good + proof.FAILURE_PREFIX.encode() + b'{}\n',
        ):
            with self.subTest(bad_type=type(bad)), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(bad)

    def test_exact_test_selection(self):
        good = (proof.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n").encode()
        proof.validate_test_listing(good)
        for bad in (b"", good + good, good.replace(b": test", b": benchmark"),
                    good.replace(proof.TEST_NAME.encode(), b"wrong"),
                    good.replace(b"1 test", b"2 tests")):
            with self.assertRaises(proof.AcceptanceError):
                proof.validate_test_listing(bad)


class FailureTests(unittest.TestCase):
    @staticmethod
    def native():
        return {"schema": 1, "acceptance": "file_structure", "success": False, "os": "linux",
                "kind": "native", "category": "native_check_failed", "stage": "dirty_outline",
                "operations_recorded": 10, "agent_owned_before": True, "server_owned_before": True,
                "document_open_before": True, "agent_reaped_after": True,
                "server_cleanup_after": "verified", "fixture_retained": True}

    def test_every_native_stage_and_cleanup_certainty(self):
        for stage in proof.NATIVE_STAGES:
            for status in ("not_started", "verified", "unverified"):
                receipt = dict(self.native(), stage=stage, server_cleanup_after=status)
                self.assertEqual(proof.validate_failure(receipt), receipt)
        for key in self.native():
            missing = self.native()
            del missing[key]
            with self.assertRaises(proof.AcceptanceError):
                proof.validate_failure(missing)
        for fields in ({"unexpected": "private"}, {"stage": "secret/path"},
                       {"operations_recorded": True}, {"operations_recorded": 12},
                       {"operations_recorded": -1}, {"agent_reaped_after": 1},
                       {"server_cleanup_after": "probably"}, {"success": True}):
            with self.assertRaises(proof.AcceptanceError):
                proof.validate_failure(dict(self.native(), **fields))

    def test_bounded_sidecar_survives_nonzero_with_no_private_output(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            native = evidence / "native-failure.json"
            native.write_text(json.dumps(self.native()), encoding="ascii")
            output = io.StringIO()
            with redirect_stdout(output):
                receipt = proof.report_failure(evidence, "runtime")
            self.assertEqual(receipt, self.native())
            self.assertEqual(json.loads((evidence / "driver-failure.json").read_text()), receipt)
            self.assertIn(proof.FAILURE_PREFIX, output.getvalue())
            self.assertNotIn(directory, output.getvalue())
            for invalid in (b'{"secret":"PRIVATE_PATH"}', b"x" * (proof.MAX_FAILURE_BYTES + 1),
                            json.dumps(self.native()).replace('"schema": 1', '"schema": 1, "schema": 1').encode()):
                native.write_bytes(invalid)
                output = io.StringIO()
                with redirect_stdout(output):
                    receipt = proof.report_failure(evidence, "runtime")
                self.assertEqual(receipt["category"], "native_receipt_invalid")
                self.assertEqual(receipt["native_cleanup"], "unmeasured")
                self.assertNotIn("PRIVATE_PATH", output.getvalue())
            native.unlink()
            with redirect_stdout(io.StringIO()):
                receipt = proof.report_failure(evidence, "runtime")
            self.assertEqual(receipt["category"], "native_receipt_missing")

    def test_setup_compile_selection_and_runtime_failure_have_fixed_evidence(self):
        for stage in proof.DRIVER_STAGES:
            with redirect_stdout(io.StringIO()):
                receipt = proof.report_failure(None, stage)
            self.assertEqual(receipt["stage"], stage)
            self.assertFalse(receipt["success"])
            self.assertEqual(receipt["native_cleanup"], "unmeasured")


class DriverTests(unittest.TestCase):
    def test_ordinary_prebuilt_agent_mock_exact_artifact_and_one_test(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            release = root / "target/release"
            release.mkdir(parents=True)
            agent, fixture = release / "cedar-agent", release / "cedar-mock-lsp"
            agent.write_bytes(b"normal shipping agent")
            fixture.write_bytes(b"existing test-only mock")
            executable = root / "target/debug/deps/cedar_app-123abc"
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"one libtest artifact")
            artifact = {"reason": "compiler-artifact", "manifest_path": str(root / "crates/app/Cargo.toml"),
                        "target": {"name": "cedar_app", "kind": ["lib"],
                                   "src_path": str(root / "crates/app/src/lib.rs")},
                        "profile": {"test": True}, "executable": str(executable)}
            listing = (proof.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n").encode()
            responses = [json.dumps(artifact).encode(), listing, payload()]
            with mock.patch.object(proof.sys, "platform", "linux"), \
                    mock.patch.object(proof, "bounded_process", side_effect=responses) as process, \
                    redirect_stdout(io.StringIO()):
                self.assertEqual(proof.run(root), proof.expected_receipt())
            self.assertEqual(process.call_count, 3)
            for call, timeout in zip(process.call_args_list, (120, 15, 420)):
                command, actual_root, environment, actual_timeout, _ = call.args
                self.assertEqual(actual_root, root)
                self.assertEqual(actual_timeout, timeout)
                self.assertEqual(environment["CEDAR_AGENT_BIN"], str(agent))
                self.assertEqual(environment["CEDAR_MOCK_LSP_BIN"], str(fixture))
                self.assertNotIn("--allow-run", command)
            self.assertEqual(process.call_args_list[1].args[0],
                             [str(executable), proof.TEST_NAME, "--ignored", "--exact", "--list"])
            self.assertEqual(process.call_args_list[2].args[0],
                             [str(executable), proof.TEST_NAME, "--ignored", "--exact",
                              "--test-threads=1", "--nocapture"])
            with mock.patch.object(proof.sys, "platform", "linux"), \
                    mock.patch.object(proof, "bounded_process", side_effect=responses), \
                    mock.patch.object(proof, "digest", side_effect=["a", "b", "c", "a", "b", "changed"]), \
                    redirect_stdout(io.StringIO()), \
                    self.assertRaisesRegex(proof.AcceptanceError, "selected_binary_changed"):
                proof.run(root)
            for stage in range(3):
                failed = responses[:stage] + [proof.AcceptanceError("subprocess_nonzero")]
                with mock.patch.object(proof.sys, "platform", "linux"), \
                        mock.patch.object(proof, "bounded_process", side_effect=failed) as process, \
                        redirect_stdout(io.StringIO()), \
                        self.assertRaisesRegex(proof.AcceptanceError, "subprocess_nonzero"):
                    proof.run(root)
                self.assertEqual(process.call_count, stage + 1, "failed proof must not retry")
            def native_failure(command, _root, environment, _timeout, _limit):
                if command == proof.test_compile_command():
                    return responses[0]
                if command[-1] == "--list":
                    return responses[1]
                Path(environment["CEDAR_FILE_STRUCTURE_FAILURE_FILE"]).write_text(
                    json.dumps(FailureTests.native()), encoding="ascii")
                raise proof.AcceptanceError("subprocess_nonzero")

            output = io.StringIO()
            with mock.patch.object(proof.sys, "platform", "linux"), \
                    mock.patch.object(proof, "bounded_process", side_effect=native_failure) as process, \
                    redirect_stdout(output), self.assertRaises(proof.AcceptanceError):
                proof.run(root)
            self.assertEqual(process.call_count, 3)
            self.assertEqual(proof.validate_failure(json.loads(
                output.getvalue().removeprefix(proof.FAILURE_PREFIX))), FailureTests.native())


if __name__ == "__main__":
    unittest.main()
