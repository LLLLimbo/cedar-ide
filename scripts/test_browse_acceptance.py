#!/usr/bin/env python3
"""Pure artifact, receipt and bounded-driver mocks; never build or run Rust."""
from contextlib import redirect_stderr, redirect_stdout
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import browse_acceptance as proof


def artifacts(root, platform="linux"):
    suffix = ".exe" if platform == "windows" else ""
    output = []
    for name, crate, kind, source in (
        ("stdio_roundtrip", "client", ["test"], "tests/stdio_roundtrip.rs"),
        ("cedar_app", "app", ["lib"], "src/lib.rs"),
    ):
        executable = root / "target/debug/deps" / (name + "-abcdef" + suffix)
        executable.parent.mkdir(parents=True, exist_ok=True)
        executable.write_bytes(b"never executed")
        output.append({"reason": "compiler-artifact",
                       "manifest_path": str(root / "crates" / crate / "Cargo.toml"),
                       "target": {"name": name, "kind": kind,
                                  "src_path": str(root / "crates" / crate / source)},
                       "profile": {"test": True}, "executable": str(executable)})
    return output


def encoded_artifacts(events):
    return ("\n".join(json.dumps(event) for event in events) + "\n").encode()


def execution(name, receipt=None):
    text = "running 1 test\ntest " + name + " ... generated fixture retained until close\n"
    if name == proof.APP_TEST:
        text += proof.PREFIX + json.dumps(proof.expected_receipt() if receipt is None else receipt) + "\n"
    return (text + "ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "999 filtered out; finished in 0.01s\n").encode()


class ArtifactTests(unittest.TestCase):
    def test_exact_artifacts_on_both_platforms(self):
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                events = artifacts(root, platform)
                normal_lib = copy.deepcopy(events[1])
                normal_lib["profile"]["test"] = False
                normal_lib["executable"] = None
                selected = proof.select_artifacts(encoded_artifacts([normal_lib] + events), root, platform)
                self.assertEqual(selected, {event["target"]["name"]: Path(event["executable"]) for event in events})

    def test_missing_duplicate_and_wrong_artifact_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            events = artifacts(root)
            invalid = [[], events[:1], events[1:], events + [events[0]]]
            for target in (0, 1):
                for field, value in (("manifest_path", "/wrong/Cargo.toml"),
                                     ("executable", None), ("executable", "/wrong/test-abcdef"),
                                     ("profile", {"test": 1}), ("profile", {"test": False})):
                    changed = copy.deepcopy(events)
                    changed[target][field] = value
                    invalid.append(changed)
                for field, value in (("kind", ["bin"]), ("src_path", "/wrong/source.rs"),
                                     ("name", "other_test")):
                    changed = copy.deepcopy(events)
                    changed[target]["target"][field] = value
                    invalid.append(changed)
            for events in invalid:
                with self.subTest(events=events), self.assertRaises(proof.AcceptanceError):
                    proof.select_artifacts(encoded_artifacts(events), root, "linux")

    def test_missing_file_and_symlink_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            events = artifacts(root)
            executable = Path(events[0]["executable"])
            executable.unlink()
            with self.assertRaises(proof.AcceptanceError):
                proof.select_artifacts(encoded_artifacts(events), root, "linux")
            try:
                executable.symlink_to(Path(events[1]["executable"]))
            except OSError:
                return  # Windows runners need not grant symlink creation.
            with self.assertRaises(proof.AcceptanceError):
                proof.select_artifacts(encoded_artifacts(events), root, "linux")


class ReceiptTests(unittest.TestCase):
    def test_exact_success_and_literal_source_hashes(self):
        for name in (proof.BACKEND_TEST, proof.APP_TEST):
            proof.validate_execution(execution(name), name)
        receipt = proof.expected_receipt()
        self.assertEqual(len(receipt["operation_ledger"]), 7)
        self.assertEqual({key: receipt[key] for key in (
            "retained_text_snapshot", "grouped_original_indices",
            "text_open_back_exact", "reopen_keeps_selection",
            "grouped_navigation_additional_operations",
            "retained_dirty_preview_additional_reads",
        )}, {
            "retained_text_snapshot": True, "grouped_original_indices": True,
            "text_open_back_exact": True, "reopen_keeps_selection": True,
            "grouped_navigation_additional_operations": 0,
            "retained_dirty_preview_additional_reads": 0,
        })
        self.assertEqual(receipt["source_sha256"], [
            ["origin.txt", "2310717b7af21074597cd38d20ed4c76988de26c130e6e729d5968891892f401"],
            [proof.TARGET, "9871cde2b808033d4b8a4252cf0360136904bc65016352ea7cfa1ce0db69c6f6"],
        ])

    def test_wrong_missing_extra_and_nested_typed_values_refused(self):
        receipt = proof.expected_receipt()
        invalid = [{**receipt, "extra": True}]
        for key, value in receipt.items():
            invalid.append({name: item for name, item in receipt.items() if name != key})
            invalid.append({**receipt, key: "wrong"})
            if type(value) is int:
                invalid.append({**receipt, key: float(value)})
            elif type(value) is bool:
                invalid.append({**receipt, key: not value})
                invalid.append({**receipt, key: int(value)})
        invalid.append({**receipt, "trust": 0})
        for change in ("ledger", "hash", "read_count", "nested_type"):
            changed = copy.deepcopy(receipt)
            if change == "ledger":
                changed["operation_ledger"].reverse()
            elif change == "hash":
                changed["source_sha256"][0][1] = "0" * 64
            elif change == "read_count":
                changed["operation_ledger"].pop()
            else:
                changed["operation_ledger"][3]["limit"] = 100.0
            invalid.append(changed)
        for value in invalid:
            with self.subTest(receipt=value), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(execution(proof.APP_TEST, value), proof.APP_TEST)

    def test_exact_test_and_single_strict_json_receipt_required(self):
        good = execution(proof.APP_TEST)
        extra = (proof.PREFIX + json.dumps(proof.expected_receipt()) + "\n").encode()
        invalid = [good.replace(b"running 1 test", b"running 0 tests"),
                   good.replace(b"1 passed", b"0 passed"),
                   good.replace(proof.APP_TEST.encode(), b"wrong_test"),
                   good + extra, good.replace(proof.PREFIX.encode(), b"missing "),
                   good.replace(b'"connections": 1', b'"connections": 1, "connections": 1'),
                   good.replace(b'"connections": 1', b'"connections": NaN')]
        for output in invalid:
            with self.subTest(output=output), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(output, proof.APP_TEST)


class DriverTests(unittest.TestCase):
    def run_fixture(self, platform="linux", failure=None, invalid_output=None, missing_agent=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            events = artifacts(root, platform)
            agent = root / "target/release" / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
            if not missing_agent:
                agent.parent.mkdir(parents=True)
                agent.write_bytes(b"never executed")
            outputs = [encoded_artifacts(events), execution(proof.BACKEND_TEST), execution(proof.APP_TEST)]
            calls = []

            def runner(command, cwd, environment, timeout, limit):
                index = len(calls)
                calls.append((command, cwd, environment, timeout, limit))
                if failure and index == failure[0]:
                    raise proof.CapturedFailure(failure[1], b"bounded useful failure\n", returncode=9,
                                                cleanup="reaped")
                return invalid_output[1] if invalid_output and index == invalid_output[0] else outputs[index]

            stdout, stderr = io.StringIO(), io.StringIO()
            with mock.patch.object(proof.sys, "platform", "win32" if platform == "windows" else "linux"), \
                    redirect_stdout(stdout), redirect_stderr(stderr):
                try:
                    proof.run(root, runner)
                except proof.AcceptanceError as error:
                    return calls, stdout.getvalue(), stderr.getvalue(), str(error)
            return calls, stdout.getvalue(), stderr.getvalue(), None

    def test_feature_graph_exact_commands_bounds_and_agent_selection(self):
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                calls, stdout, stderr, error = self.run_fixture(platform)
                self.assertIsNone(error)
                self.assertEqual(stderr, "")
                self.assertEqual(calls[0][0], ["cargo", "test", "--workspace", "--all-targets", "--all-features",
                                             "--locked", "--no-run", "--message-format=json"])
                self.assertEqual([call[3] for call in calls], [120, 180, 330])
                self.assertEqual([call[4] for call in calls], [4 * 1024 * 1024, 256 * 1024, 256 * 1024])
                for call, name, variable in ((calls[1], proof.BACKEND_TEST, "CEDAR_AGENT_BIN"),
                                              (calls[2], proof.APP_TEST, "CEDAR_BROWSE_AGENT_BIN")):
                    self.assertEqual(call[0][1:], [name, "--ignored", "--exact", "--test-threads=1", "--nocapture"])
                    suffix = ".exe" if platform == "windows" else ""
                    self.assertEqual(call[2][variable], str(call[1] / "target/release" / ("cedar-agent" + suffix)))
                self.assertEqual(stdout.count(proof.PREFIX), 1)

    def test_nonzero_timeout_and_output_limit_stop_without_retry(self):
        for stage in range(3):
            for category in ("subprocess_nonzero", "subprocess_timeout", "subprocess_output_limit"):
                with self.subTest(stage=stage, category=category):
                    calls, _, stderr, error = self.run_fixture(failure=(stage, category))
                    self.assertEqual(len(calls), stage + 1)
                    self.assertIn(category, error)
                    self.assertIn("native_cleanup=unverified", error)
                    self.assertIn("bounded useful failure", stderr)

    def test_bad_artifact_and_receipt_stop_without_retry(self):
        for stage, output in ((0, b""), (1, b"running 0 tests\n"),
                              (2, execution(proof.APP_TEST, {"wrong": True}))):
            with self.subTest(stage=stage):
                calls, _, _, error = self.run_fixture(invalid_output=(stage, output))
                self.assertEqual(len(calls), stage + 1)
                self.assertIn("native_cleanup=unverified", error)

    def test_missing_agent_stops_before_compilation(self):
        calls, _, _, error = self.run_fixture(missing_agent=True)
        self.assertEqual(calls, [])
        self.assertEqual(error, "normal_release_agent_missing")


if __name__ == "__main__":
    unittest.main()
