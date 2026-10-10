#!/usr/bin/env python3
"""Strict fixed receipt checks; no process launch, SSH or deployment."""
import contextlib
import copy
import io
import json
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import linux_desktop_bundle_acceptance as acceptance


def valid():
    return {
        **{key: True for key in acceptance.BOOLS}, **acceptance.FIXED,
        "kind": "cedar_linux_desktop_bundle_probe", "status": "success",
        "explicit_client_calls": 76, "elapsed_ms": 123,
    }


class ReceiptTests(unittest.TestCase):
    def test_complete_receipt_and_exact_elapsed_boundary(self):
        for elapsed in (0, 30000):
            receipt = valid()
            receipt["elapsed_ms"] = elapsed
            self.assertEqual(acceptance.validate_probe(receipt), receipt)
            output = b"running 1 test\n" + json.dumps(receipt).encode() + b"\ntest result: ok\n"
            self.assertEqual(acceptance.parse_probe(output), receipt)

    def test_every_required_witness_is_boolean_true(self):
        for key in acceptance.BOOLS:
            for replacement in (False, None, 1, "true", [], {}):
                with self.subTest(key=key, replacement=replacement):
                    receipt = valid()
                    receipt[key] = replacement
                    with self.assertRaises(ValueError):
                        acceptance.validate_probe(receipt)

    def test_missing_extra_and_wrong_root_shapes_rejected(self):
        for key in valid():
            receipt = valid()
            del receipt[key]
            with self.subTest(key=key), self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)
        for receipt in (None, [], [valid()], [[valid()]], True, "success",
                        {**valid(), "private_extra": "discarded"}):
            with self.subTest(receipt=type(receipt).__name__), self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)

    def test_obsolete_unadvertised_java_or_maven_witness_is_rejected(self):
        for obsolete in ("windows_java_operations_unadvertised",
                         "typed_java_advertised_maven_unadvertised", "maven_unadvertised"):
            receipt = valid()
            del receipt["maven_groups_advertised"]
            receipt[obsolete] = True
            with self.subTest(obsolete=obsolete), self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)
            with self.assertRaises(ValueError):
                acceptance.validate_probe({**valid(), obsolete: True})

    def test_exact_flat_and_group_counts_cannot_hide_inventory_drift(self):
        for field, values in (("capability_count", (0, 1, 30, 32, 33)),
                              ("capability_group_count", (0, 1, 3, 31))):
            for value in values:
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    acceptance.validate_probe({**valid(), field: value})

    def test_fixed_integer_identity_and_budgets_do_not_coerce(self):
        for key, expected in acceptance.FIXED.items():
            for value in (True, False, str(expected), float(expected), expected + 1, None):
                receipt = valid()
                receipt[key] = value
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_probe(receipt)

    def test_bounded_counts_late_results_and_nonfinite_values_rejected(self):
        wrong = {
            "capability_count": (0, 33, -1, True, 1.0, "31"),
            "explicit_client_calls": (0, 97, -1, True, 76.0, "76"),
            "elapsed_ms": (-1, 30001, True, 0.0, float("nan"), float("inf")),
        }
        for key, values in wrong.items():
            for value in values:
                receipt = valid()
                receipt[key] = value
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_probe(receipt)
        receipt = valid()
        receipt["elapsed_ms"] = float("nan")
        with self.assertRaises(ValueError):
            acceptance.parse_probe(json.dumps(receipt).encode())

    def test_failure_and_wrong_kind_rejected(self):
        for key, value in (("kind", "other"), ("status", "failed"), ("status", True)):
            receipt = valid()
            receipt[key] = value
            with self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)

    def test_duplicate_receipts_keys_malformed_and_overflow_rejected(self):
        encoded = json.dumps(valid()).encode()
        for output in (b"", b"running no receipt", encoded + b"\n" + encoded,
                       encoded[:-1] + b', "status": "success"}', b"{broken}",
                       b"\xff", b"x" * (acceptance.MAX_TEST_OUTPUT + 1)):
            with self.subTest(size=len(output)), self.assertRaises((ValueError, UnicodeError)):
                acceptance.parse_probe(output)

    def test_gate_validates_build_host_before_touching_output(self):
        with mock.patch.object(acceptance.bundle, "require_build_host",
                               side_effect=ValueError("unsupported host")):
            with mock.patch.object(acceptance.Path, "resolve") as resolve:
                with self.assertRaisesRegex(ValueError, "unsupported host"):
                    acceptance.run("unused", "unused", "a" * 40,
                                   "https://github.com/LLLLimbo/cedar-ide/actions/runs/1")
                resolve.assert_not_called()

    def test_validation_does_not_change_receipt(self):
        receipt = valid()
        before = copy.deepcopy(receipt)
        acceptance.validate_probe(receipt)
        self.assertEqual(receipt, before)


def valid_rejections():
    return {
        **{key: True for key in acceptance.REJECTION_BOOLS},
        **acceptance.REJECTION_FIXED,
        "kind": "cedar_linux_desktop_rejection_suite", "status": "success", "elapsed_ms": 123,
    }


class RejectionReceiptTests(unittest.TestCase):
    def test_exact_receipt_and_bounds(self):
        for elapsed in (0, 90000):
            receipt = {**valid_rejections(), "elapsed_ms": elapsed}
            self.assertEqual(acceptance.validate_rejections(receipt), receipt)
            self.assertEqual(acceptance.parse_receipt(json.dumps(receipt).encode(),
                                                     acceptance.validate_rejections), receipt)

    def test_missing_extra_array_and_wrong_kind(self):
        for key in valid_rejections():
            receipt = valid_rejections()
            del receipt[key]
            with self.subTest(key=key), self.assertRaises(ValueError):
                acceptance.validate_rejections(receipt)
        for receipt in ([], [valid_rejections()], None, True, "success",
                        {**valid_rejections(), "private": "never retain"},
                        {**valid_rejections(), "kind": "cedar_linux_desktop_bundle_probe"},
                        {**valid_rejections(), "status": "failure"}):
            with self.subTest(receipt=type(receipt).__name__), self.assertRaises(ValueError):
                acceptance.validate_rejections(receipt)

    def test_every_boolean_and_bound_requires_exact_type(self):
        for key in acceptance.REJECTION_BOOLS:
            for value in (False, None, 1, "true", [], {}):
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_rejections({**valid_rejections(), key: value})
        for key, expected in acceptance.REJECTION_FIXED.items():
            for value in (True, False, str(expected), float(expected), expected + 1, None):
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_rejections({**valid_rejections(), key: value})
        for value in (-1, 90001, True, 1.0, "1", None, float("nan")):
            with self.subTest(value=value), self.assertRaises(ValueError):
                acceptance.validate_rejections({**valid_rejections(), "elapsed_ms": value})

    def test_duplicate_malformed_oversized_and_array_outputs(self):
        encoded = json.dumps(valid_rejections()).encode()
        for data in (b"", b"[]", b"[" + encoded + b"]", encoded + b"\n" + encoded,
                     encoded[:-1] + b', "cases": 20}', b"{broken}", b"\xff",
                     b"x" * (acceptance.MAX_TEST_OUTPUT + 1)):
            with self.subTest(size=len(data)), self.assertRaises((ValueError, UnicodeError)):
                acceptance.parse_receipt(data, acceptance.validate_rejections)
        with self.assertRaises(ValueError):
            acceptance.parse_receipt(encoded.replace(b'"elapsed_ms": 123', b'"elapsed_ms": NaN'),
                                     acceptance.validate_rejections)


class ProcessBoundaryTests(unittest.TestCase):
    def fake_supervisor(self, root, data, exit_code=0, times=None, already_exited=False):
        command = ["owned-probe", "portable", "generated root"]
        pipe = mock.Mock()
        pipe.fileno.return_value = 321
        process = mock.Mock(stdout=pipe)
        process.returncode = exit_code if already_exited else None
        process.poll.side_effect = lambda: process.returncode
        process.kill.side_effect = lambda: setattr(process, "returncode", -9)
        selector = mock.MagicMock()
        selector.__enter__.return_value = selector
        selector.select.return_value = [(SimpleNamespace(fd=321), 1)]
        remaining = bytearray(data)
        read_sizes = []

        def read(descriptor, limit):
            self.assertEqual(descriptor, 321)
            self.assertLessEqual(limit, 65536)
            read_sizes.append(limit)
            chunk = bytes(remaining[:limit])
            del remaining[:limit]
            if not chunk:
                process.returncode = exit_code
            return chunk

        clock = {"side_effect": times} if times is not None else {"return_value": 0}
        with mock.patch.object(acceptance.subprocess, "Popen", return_value=process) as launch, \
                mock.patch.object(acceptance.os, "set_blocking") as nonblocking, \
                mock.patch.object(acceptance.os, "read", side_effect=read), \
                mock.patch.object(acceptance.selectors, "DefaultSelector", return_value=selector), \
                mock.patch.object(acceptance.time, "monotonic", **clock):
            try:
                result = acceptance.run_process(command, root, root / "private.log", 45)
            finally:
                launch.assert_called_once_with(command, cwd=root, stdin=subprocess.DEVNULL,
                                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                nonblocking.assert_called_once_with(321, False)
                process.wait.assert_called_once_with(timeout=5)
                pipe.close.assert_called_once()
                self.last_process = process
                self.last_read_sizes = read_sizes
        return result

    def test_exact_arguments_private_output_nonblocking_and_no_shell(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertEqual(self.fake_supervisor(root, b"controlled output\n"),
                             b"controlled output\n")
            self.last_process.kill.assert_not_called()
            with mock.patch.object(acceptance.subprocess, "Popen") as launch:
                with self.assertRaises(FileExistsError):
                    acceptance.run_process(["probe"], root, root / "private.log", 45)
                launch.assert_not_called()

    def test_exact_output_cap_succeeds_without_extra_retained_byte(self):
        with tempfile.TemporaryDirectory() as directory:
            data = b"x" * acceptance.MAX_TEST_OUTPUT
            self.assertEqual(self.fake_supervisor(Path(directory), data), data)
            self.assertEqual(self.last_read_sizes[-1], 1)
            self.last_process.kill.assert_not_called()

    def test_overflow_is_stopped_during_drain_and_never_written(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, "output exceeded bound"):
                self.fake_supervisor(root, b"x" * (acceptance.MAX_TEST_OUTPUT + 5000))
            self.assertLessEqual((root / "private.log").stat().st_size, acceptance.MAX_TEST_OUTPUT)
            self.assertEqual(self.last_read_sizes[-1], 1)
            self.last_process.kill.assert_called_once_with()

    def test_nonzero_cannot_be_a_success_or_cleanup_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "Probe process failed"):
                self.fake_supervisor(Path(directory), b"private diagnostic", exit_code=1)
            self.last_process.kill.assert_not_called()

    def test_deadline_kills_only_the_owned_running_direct_child(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "agent cleanup unverified") as error:
                self.fake_supervisor(Path(directory), b"private root", times=[0, 46])
            self.assertNotIn("private root", str(error.exception))
            self.last_process.kill.assert_called_once_with()

    def test_root_exit_does_not_remove_original_pipe_drain_deadline(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "agent cleanup unverified"):
                self.fake_supervisor(Path(directory), b"inherited pipe output", times=[0, 46],
                                     already_exited=True)
            self.last_process.kill.assert_not_called()


class AcceptanceLifecycleTests(unittest.TestCase):
    def run_fixture(self, directory, fail=False):
        base = Path(directory)
        root = base / "repository"
        scratch_root = base / "scratch"
        (root / "target/release").mkdir(parents=True)
        scratch_root.mkdir()
        (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.41.0"\n')
        (root / "target/release" / acceptance.PROBE_NAME).write_bytes(b"\x7fELF\x02\x01\x01probe")
        payload = {"cedar": b"desktop", "cedar-agent": b"agent", "BUNDLE_MANIFEST.json": b"{}"}
        source_commit = "a" * 40
        ci_url = "https://github.com/LLLLimbo/cedar-ide/actions/runs/1"
        extracted_checks = []
        executions = []

        def fake_build(repo, binaries, archive, commit, run_url):
            self.assertEqual((repo, binaries, commit, run_url),
                             (root, root / "target/release", source_commit, ci_url))
            archive.write_bytes(b"controlled archive")
            return {"abi": {"cedar": {}, "cedar-agent": {}}}

        def fake_extract(actual, destination):
            self.assertEqual(actual, payload)
            destination.mkdir()
            for name, data in payload.items():
                (destination / name).write_bytes(data)

        def fake_verify(actual, destination):
            self.assertEqual(actual, payload)
            self.assertEqual({path.name for path in destination.iterdir()}, set(payload))
            self.assertEqual({path.name: path.read_bytes() for path in destination.iterdir()}, payload)
            extracted_checks.append(True)

        def fake_process(command, cwd, log, timeout):
            executions.append(command)
            if command[0] == "cargo":
                self.assertIn("--offline", command)
                self.assertIn("--locked", command)
                self.assertIn("--release", command)
                self.assertEqual(command[command.index("--features") + 1], "fixtures")
                self.assertEqual(command[command.index("--exact") + 1], acceptance.TEST_NAME)
                return json.dumps(valid_rejections()).encode()
            self.assertEqual(command[1], "portable")
            self.assertEqual(Path(command[0]).parent.name, "renamed Cedar desktop 雪 with spaces")
            if fail:
                raise ValueError("controlled probe failure")
            workspace = Path(command[2])
            (workspace / acceptance.FILE).write_bytes(acceptance.SAVED)
            return json.dumps(valid()).encode()

        with mock.patch.object(acceptance.bundle, "require_build_host"), \
                mock.patch.object(acceptance.bundle, "build", side_effect=fake_build), \
                mock.patch.object(acceptance.bundle, "verify_bytes", return_value=({}, payload)), \
                mock.patch.object(acceptance.bundle, "verify_source") as source_check, \
                mock.patch.object(acceptance.bundle, "extract_payload", side_effect=fake_extract), \
                mock.patch.object(acceptance.bundle, "verify_extracted", side_effect=fake_verify), \
                mock.patch.object(acceptance, "run_process", side_effect=fake_process), \
                contextlib.redirect_stdout(io.StringIO()):
            if fail:
                with self.assertRaisesRegex(ValueError, "controlled probe failure"):
                    acceptance.run(root, scratch_root, source_commit, ci_url)
                result = None
                self.assertEqual(source_check.call_count, 1)
            else:
                result = acceptance.run(root, scratch_root, source_commit, ci_url)
                self.assertEqual(source_check.call_count, 2)
                self.assertEqual(len(extracted_checks), 3)
                self.assertEqual(len(executions), 2)
        remaining = list(scratch_root.iterdir())
        self.assertEqual([path.name for path in remaining], ["cedar-linux-desktop-development"])
        receipt = remaining[0] / "BUNDLE_VERIFICATION.json"
        self.assertEqual(receipt.exists(), not fail)
        self.assertEqual(next(remaining[0].glob("*.tar.gz")).read_bytes(), b"controlled archive")
        return result

    def test_success_removes_probe_before_package_rechecks_and_records_no_gui(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_fixture(directory)
        self.assertTrue(result["probe_excluded_from_archive"])
        self.assertTrue(result["payload_unchanged"])
        self.assertTrue(result["scratch_removed"])
        for key in ("gui_exercised", "native_desktop_acceptance", "authenticated_ssh_exercised",
                    "network_exercised", "deployment_performed"):
            self.assertIs(result[key], False)
        self.assertEqual(set(result["binaries"]), {"cedar", "cedar-agent"})

    def test_failed_probe_removes_only_owned_scratch_and_never_emits_success(self):
        with tempfile.TemporaryDirectory() as directory:
            self.run_fixture(directory, fail=True)


if __name__ == "__main__":
    unittest.main()
