#!/usr/bin/env python3
"""Strict predicates and short generated-process supervision tests. No JDT or download."""
import copy
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
import tarfile
import unittest
from unittest import mock

import linux_java_acceptance as acceptance


def stop():
    return {"platform": "linux", "status": "graceful", "reason": "root_exited",
            "root_exit": {"kind": "code", "code": 0}, "cleanup_joined": True,
            "shutdown_response_received": True, "exit_frame_completed": True}


def valid():
    return {**{key: True for key in acceptance.BOOLS},
            **{key: False for key in acceptance.FALSE_BOOLS}, **acceptance.FIXED, **acceptance.ENUMS,
            "organize_main_edits": 1, "organize_ambiguity_edits": 1,
            "elapsed_ms": 3000, "main_elapsed_ms": 1000, "restart_elapsed_ms": 1000,
            "initial_stop": stop(), "restart_stop": stop(),
            "spontaneous_result": "matched", "spontaneous_success": True,
            "recovery_attempts": 0, "recovery_result": "not_attempted",
            "recovery_acknowledged": False, "recovery_witness": False,
            "recovery_unversioned": False, "recovery_budget_sufficient": False}


def recovered():
    return {**valid(), "spontaneous_result": "timeout", "spontaneous_success": False,
            "recovery_attempts": 1, "recovery_result": "matched", "recovery_acknowledged": True,
            "recovery_witness": True, "recovery_unversioned": True, "recovery_budget_sufficient": True}


class ProbeTests(unittest.TestCase):
    def test_shared_fixture_feature_is_enabled_only_for_app_library_test_build(self):
        self.assertEqual(acceptance.test_compile_command(), [
            "cargo", "test", "-p", "cedar-app", "--lib", "--locked", "--offline",
            "--features", "windows-language-validation", "--no-run", "--message-format=json"])

    def test_exact_complete_receipt_and_distinct_recovered_workflow(self):
        for receipt in (valid(), recovered()):
            before = copy.deepcopy(receipt)
            self.assertEqual(acceptance.validate_probe(receipt), before)
            self.assertEqual(receipt, before)
            data = b"running 1 test\n" + json.dumps(receipt).encode() + b"\ntest result: ok\n"
            self.assertEqual(acceptance.parse_probe(data), before)
        self.assertFalse(acceptance.validate_probe(recovered())["spontaneous_success"])

    def test_required_witnesses_and_failure_flags_are_exact_booleans(self):
        for keys, expected in ((acceptance.BOOLS, True), (acceptance.FALSE_BOOLS, False)):
            for key in keys:
                for value in (not expected, None, 1, 0, "true", [], {}):
                    receipt = valid()
                    receipt[key] = value
                    with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                        acceptance.validate_probe(receipt)

    def test_group_advertisement_and_trust_off_maven_replace_obsolete_witness(self):
        receipt = valid()
        self.assertIs(receipt["maven_groups_advertised"], True)
        self.assertIs(receipt["maven_trust_off_rejected"], True)
        self.assertEqual(receipt["capability_count"], 31)
        self.assertEqual(receipt["capability_group_count"], 2)
        del receipt["maven_groups_advertised"]
        receipt["maven_unadvertised"] = True
        with self.assertRaises(ValueError):
            acceptance.validate_probe(receipt)
        with self.assertRaises(ValueError):
            acceptance.decode_probe(json.dumps(receipt).encode())
        with self.assertRaises(ValueError):
            acceptance.validate_probe({**valid(), "maven_unadvertised": True})

    def test_missing_extra_and_wrong_schema_rejected(self):
        for key in valid():
            receipt = valid()
            del receipt[key]
            with self.subTest(key=key), self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)
        for receipt in (None, [], [valid()], True, "success", {**valid(), "raw_private": "sentinel"}):
            with self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)

    def test_fixed_integer_bounds_and_enums_do_not_coerce(self):
        for key, expected in acceptance.FIXED.items():
            for value in (True, False, str(expected), float(expected), expected + 1, None):
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_probe({**valid(), key: value})
        for key in acceptance.ENUMS:
            for value in ("other", None, True, 1):
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_probe({**valid(), key: value})

    def test_counts_late_time_and_nonfinite_values_rejected(self):
        for key, (low, high) in acceptance.COUNTS.items():
            for value in (low - 1, high + 1, True, 1.0, "1", None, float("nan"), float("inf")):
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_probe({**valid(), key: value})
        with self.assertRaises(ValueError):
            acceptance.validate_probe({**valid(), "elapsed_ms": 1999})
        with self.assertRaises(ValueError):
            acceptance.parse_probe(json.dumps({**valid(), "elapsed_ms": float("nan")}).encode())

    def test_recovery_never_erases_spontaneous_failure_or_accepts_other_failure(self):
        for key, value in (("spontaneous_success", True), ("recovery_attempts", 0),
                           ("recovery_attempts", 2), ("recovery_attempts", True),
                           ("recovery_result", "not_attempted"), ("recovery_acknowledged", False),
                           ("recovery_witness", False), ("recovery_budget_sufficient", False),
                           ("recovery_unversioned", 1), ("spontaneous_result", "closed"),
                           ("spontaneous_result", "malformed_events"), ("spontaneous_result", "request_error")):
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                acceptance.validate_probe({**recovered(), key: value})
        for key, value in (("recovery_attempts", 1), ("recovery_result", "matched"),
                           ("recovery_acknowledged", True), ("recovery_witness", True),
                           ("recovery_unversioned", True), ("recovery_budget_sufficient", True),
                           ("spontaneous_success", False)):
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                acceptance.validate_probe({**valid(), key: value})

    def test_receipt_needs_its_own_line_and_never_scans_private_substrings(self):
        encoded = json.dumps(valid()).encode()
        prefix = ("test " + acceptance.TEST_NAME + " ... ").encode()
        self.assertEqual(acceptance.parse_probe(prefix + b"\n" + encoded + b"\n"), valid())
        for output in (prefix + encoded + b"\n", b"private stderr contains " + encoded + b"\n"):
            with self.assertRaises(ValueError):
                acceptance.parse_probe(output)

    def test_duplicate_receipts_keys_malformed_utf8_and_output_overflow(self):
        encoded = json.dumps(valid()).encode()
        for output in (b"", b"no receipt", encoded + b"\n" + encoded,
                       encoded[:-1] + b', "status": "success"}', b"{broken}",
                       b"\xff", b"x" * (acceptance.MAX_LOG_BYTES + 1)):
            with self.subTest(size=len(output)), self.assertRaises((ValueError, UnicodeError)):
                acceptance.parse_probe(output)


    def test_failed_receipt_is_structurally_sanitized_without_passing_success_gate(self):
        receipt = {**valid(), "status": "failed", "primary_failed": True, "failure_stage": "correction",
                   "spontaneous_result": "timeout", "spontaneous_success": False,
                   "recovery_result": "insufficient_budget", "initial_stop": None, "restart_stop": None,
                   "main_elapsed_ms": 480123, "elapsed_ms": 480123, "main_deadline_met": False}
        data = json.dumps(receipt).encode()
        self.assertEqual(acceptance.decode_probe(data), receipt)
        with self.assertRaises(ValueError):
            acceptance.parse_probe(data)
        for key, value in (("failure_stage", "raw private sentinel"), ("spontaneous_result", "private message"),
                           ("capability_count", "31"), ("elapsed_ms", 0x100000000),
                           ("recovery_attempts", 2), ("initial_stop", {**stop(), "private": "sentinel"})):
            with self.subTest(key=key), self.assertRaises(ValueError):
                acceptance.decode_probe(json.dumps({**receipt, key: value}).encode())

    def test_failure_receipt_unavailable_and_malformed_categories_never_echo_raw_output(self):
        self.assertEqual(acceptance.failure_probe(None), ("not_run", None))
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "private.log"
            for data, expected in ((b"private compiler text", "unavailable"),
                                   (b"{private malformed", "malformed"),
                                   (json.dumps({**valid(), "raw_private": "sentinel"}).encode(), "malformed")):
                log.write_bytes(data)
                self.assertEqual(acceptance.failure_probe(log), (expected, None))
            receipt = {**valid(), "status": "failed", "primary_failed": True}
            log.write_text(json.dumps(receipt))
            self.assertEqual(acceptance.failure_probe(log), ("available", receipt))

class BundledWorkflowTests(unittest.TestCase):
    def exercise(self, records):
        with tempfile.TemporaryDirectory() as directory:
            scratch = Path(directory)
            calls = []
            def run(command, cwd, environment, log, timeout):
                calls.append((command, timeout))
                if "--list" in command:
                    log.write_bytes((acceptance.BUNDLED_RUN_TEST + ": test\n\n1 test, 0 benchmarks\n").encode())
                else:
                    log.write_bytes(("\n".join(json.dumps(record) for record in records) + "\n").encode())
            with mock.patch.object(acceptance, "bounded_process", side_effect=run):
                result = acceptance.run_bundled_workflow(Path("harness"), scratch, {})
            self.assertEqual([call[1] for call in calls], [15, 60])
            self.assertIn("--exact", calls[1][0])
            return result

    def test_one_executed_case_with_verified_cleanup(self):
        record = {"kind": "bundled_run_save_cancel", "cases": 1, "success": True, "cleanup_verified": True}
        self.assertEqual(self.exercise([record]), record)

    def test_missing_duplicate_forged_and_unverified_cases_fail(self):
        valid = {"kind": "bundled_run_save_cancel", "cases": 1, "success": True, "cleanup_verified": True}
        bad = [[], [valid, valid], [{**valid, "cases": True}], [{**valid, "cases": 0}],
               [{**valid, "success": 1}], [{**valid, "cleanup_verified": False}],
               [{**valid, "extra": True}], [[valid]]]
        for records in bad:
            with self.subTest(records=records), self.assertRaises(ValueError):
                self.exercise(records)


class TestSelectionTests(unittest.TestCase):
    def test_actual_aggregate_test_path_is_uniquely_listed(self):
        self.assertEqual(acceptance.TEST_NAME,
                         "language_ui::real_java_tests::acceptance::linux::real_linux_normal_agent_java_editor_acceptance")
        listing = acceptance.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n"
        acceptance.validate_test_listing(listing.encode())
        acceptance.validate_test_listing(listing.replace("\n", "\r\n").encode())

    def test_empty_wrong_duplicate_extra_and_malformed_listings_fail_before_runtime(self):
        expected = acceptance.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n"
        wrong = [b"", b"0 tests, 0 benchmarks\n", b"\xff", b"x" * (acceptance.MAX_LIST_BYTES + 1),
                 expected.replace("language_ui::real_java_tests::acceptance", "real_java_acceptance_tests").encode(),
                 expected.replace("1 test", "2 tests").encode(),
                 expected.replace(": test", ": benchmark").encode(),
                 (expected + acceptance.TEST_NAME + ": test\n").encode(),
                 ("other::test: test\n" + expected).encode(),
                 (expected + "private unexpected output\n").encode()]
        for listing in wrong:
            with self.subTest(size=len(listing)), self.assertRaises((ValueError, UnicodeError)):
                acceptance.validate_test_listing(listing)


class StopTests(unittest.TestCase):
    def test_platform_code_signal_bounds_and_natural_shutdown(self):
        self.assertEqual(acceptance.validate_stop(stop()), stop())
        for exit_value in ({"kind": "code", "code": 0}, {"kind": "code", "code": 255},
                           {"kind": "signal", "signal": 1}, {"kind": "signal", "signal": 64}):
            forced = {**stop(), "status": "forced", "reason": "grace_expired", "root_exit": exit_value}
            self.assertEqual(acceptance.validate_stop(forced), forced)
        for exit_value in ({"kind": "code", "code": -1}, {"kind": "code", "code": 256},
                           {"kind": "code", "code": True}, {"kind": "code", "code": 0.0},
                           {"kind": "signal", "signal": 0}, {"kind": "signal", "signal": 65},
                           {"kind": "signal", "signal": True}, {"kind": "signal", "signal": 9, "code": 0},
                           {"kind": "unknown"}, None, 0):
            with self.subTest(exit_value=exit_value), self.assertRaises(ValueError):
                acceptance.validate_stop({**stop(), "status": "forced", "root_exit": exit_value})
        for key, value in (("platform", "windows"), ("status", "error"), ("status", "unknown"),
                           ("reason", "grace_expired"), ("reason", "unknown"), ("cleanup_joined", False),
                           ("cleanup_joined", 1), ("shutdown_response_received", False),
                           ("exit_frame_completed", False), ("exit_frame_completed", 1),
                           ("root_exit", {"kind": "signal", "signal": 9}),
                           ("root_exit", {"kind": "code", "code": 1})):
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                acceptance.validate_stop({**stop(), key: value})

    def test_missing_extra_windows_and_unobserved_stop_rejected(self):
        for key in stop():
            receipt = stop()
            del receipt[key]
            with self.subTest(key=key), self.assertRaises(ValueError):
                acceptance.validate_stop(receipt)
        for value in ({**stop(), "root_exit_code": 0}, {**stop(), "raw_private": "sentinel"},
                      [], None, True, {**stop(), "root_exit": {"kind": "unobserved"}}):
            with self.assertRaises(ValueError):
                acceptance.validate_stop(value)
        for field in ("initial_stop", "restart_stop"):
            with self.assertRaises(ValueError):
                acceptance.validate_probe({**valid(), field: None})


class ArchiveTests(unittest.TestCase):
    def member(self, name, kind=tarfile.REGTYPE, size=0):
        member = tarfile.TarInfo(name)
        member.type = kind
        member.size = size
        return member

    def test_exact_pin_and_safe_members(self):
        self.assertEqual(acceptance.ARCHIVE_SHA256,
                         "338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64")
        self.assertEqual(acceptance.ARCHIVE_URL,
                         "https://download.eclipse.org/jdtls/milestones/1.61.0/jdt-language-server-1.61.0-202609031315.tar.gz")
        self.assertEqual(str(acceptance.archive_member(self.member("./plugins/example.jar"))), "plugins/example.jar")
        self.assertIsNone(acceptance.archive_member(self.member(".", tarfile.DIRTYPE)))

    def test_traversal_absolute_links_special_sparse_and_oversized_entries(self):
        for name in ("../escape", "/absolute", "a/../../escape", "a\\escape", "a\x00bad", "a//b", "a/./b", "x" * 4097):
            with self.subTest(name=name), self.assertRaises(ValueError):
                acceptance.archive_member(self.member(name))
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE, tarfile.BLKTYPE, tarfile.FIFOTYPE):
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                acceptance.archive_member(self.member("entry", kind))
        for size in (-1, acceptance.MAX_MEMBER_BYTES + 1):
            with self.assertRaises(ValueError):
                acceptance.archive_member(self.member("entry", size=size))
        member = self.member("sparse")
        member.sparse = [(0, 1)]
        with self.assertRaises(ValueError):
            acceptance.archive_member(member)

    def test_disk_preflight_exact_boundaries_and_no_numeric_coercion(self):
        for content in (0, acceptance.MAX_ARCHIVE_BYTES, acceptance.MAX_EXTRACTED_BYTES):
            required = content + acceptance.DISK_RESERVE_BYTES
            acceptance.disk_preflight(required, content)
            acceptance.disk_preflight(required + 1, content)
            with self.assertRaisesRegex(acceptance.DiskPreflightError, "^disk_preflight$"):
                acceptance.disk_preflight(required - 1, content)
        for free, content in ((True, 0), (-1, 0), (1.0, 0), (None, 0),
                              (0x10000000000000000, 0), (0, True), (0, -1),
                              (0, 1.0), (0, acceptance.MAX_EXTRACTED_BYTES + 1)):
            with self.subTest(free=free, content=content), self.assertRaises(ValueError):
                acceptance.disk_preflight(free, content)

    def test_prescan_counts_exact_regular_bytes_and_rejects_all_unsafe_members(self):
        members = [self.member(".", tarfile.DIRTYPE), self.member("plugins", tarfile.DIRTYPE),
                   self.member("plugins/a.jar", size=17), self.member("plugins/b.jar", size=23)]
        self.assertEqual(acceptance.scan_members(members), 40)
        for tail in (self.member("plugins/a.jar"), self.member("../escape"),
                     self.member("link", tarfile.SYMTYPE), self.member("directory", tarfile.DIRTYPE, 1)):
            with self.assertRaises(ValueError):
                acceptance.scan_members([*members, tail])
        with mock.patch.object(acceptance, "MAX_MEMBERS", 3), self.assertRaises(ValueError):
            acceptance.scan_members(members)
        with mock.patch.object(acceptance, "MAX_EXTRACTED_BYTES", 39), self.assertRaises(ValueError):
            acceptance.scan_members(members)

    def test_predownload_disk_failure_happens_before_acquisition(self):
        free = acceptance.MAX_ARCHIVE_BYTES + acceptance.DISK_RESERVE_BYTES - 1
        with mock.patch.object(acceptance.Path, "exists", return_value=False), \
                mock.patch.object(acceptance.shutil, "disk_usage", return_value=mock.Mock(free=free)), \
                mock.patch.object(acceptance.urllib.request, "urlopen") as download, \
                mock.patch.object(acceptance.Path, "open") as create:
            with self.assertRaises(acceptance.DiskPreflightError):
                acceptance.prepare("/owned/archive", "/owned/distribution")
            download.assert_not_called()
            create.assert_not_called()

    def test_expansion_disk_failure_happens_after_full_scan_before_any_extraction(self):
        members = [self.member("plugins/a.jar", size=17), self.member("plugins/b.jar", size=23)]
        archive = mock.MagicMock()
        archive.__enter__.return_value = archive
        archive.__iter__.return_value = iter(members)
        digest = mock.Mock()
        digest.hexdigest.return_value = acceptance.ARCHIVE_SHA256
        with mock.patch.object(acceptance.Path, "exists", autospec=True,
                               side_effect=lambda path: path.name == "archive"), \
                mock.patch.object(acceptance, "read_regular", return_value=b"synthetic verified archive"), \
                mock.patch.object(acceptance.hashlib, "sha256", return_value=digest), \
                mock.patch.object(acceptance.tarfile, "open", return_value=archive) as open_tar, \
                mock.patch.object(acceptance.shutil, "disk_usage", return_value=mock.Mock(
                    free=40 + acceptance.DISK_RESERVE_BYTES - 1)), \
                mock.patch.object(acceptance.Path, "mkdir") as mkdir:
            with self.assertRaises(acceptance.DiskPreflightError):
                acceptance.prepare("/owned/archive", "/owned/distribution")
            open_tar.assert_called_once()
            archive.extractfile.assert_not_called()
            mkdir.assert_not_called()

    def test_wrong_hash_rejected_before_archive_extraction(self):
        with mock.patch.object(acceptance.Path, "exists", return_value=True), \
                mock.patch.object(acceptance, "read_regular", return_value=b"wrong pinned archive"), \
                mock.patch.object(acceptance.tarfile, "open") as extract:
            with self.assertRaisesRegex(ValueError, "checksum"):
                acceptance.prepare("unused", "unused")
            extract.assert_not_called()

    def test_non_linux_host_rejected_before_paths_or_processes(self):
        with mock.patch.object(acceptance.sys, "platform", "win32"), \
                mock.patch.object(acceptance.Path, "resolve") as resolve, \
                mock.patch.object(acceptance, "bounded_process") as launch:
            with self.assertRaises(ValueError):
                acceptance.run("unused", "unused", "unused", "unused")
            resolve.assert_not_called()
            launch.assert_not_called()


@unittest.skipUnless(sys.platform == "linux", "Linux waitid ownership contract")
class SupervisorTests(unittest.TestCase):
    def run_helper(self, source, timeout, cap):
        # Only this fresh helper's session is ever signalled. Capture the exact
        # live Popen object before supervision; never rediscover an old PID.
        processes = []
        signals = []
        real_popen = subprocess.Popen
        real_killpg = os.killpg

        def spawn(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            processes.append(process)
            return process

        def terminate(group, sig):
            self.assertEqual(len(processes), 1)
            self.assertEqual(group, processes[0].pid)
            self.assertEqual(sig, signal.SIGKILL)
            # WNOWAIT must retain the original direct child even when it
            # already exited; loss of wait ownership forbids this signal.
            observed = os.waitid(os.P_PID, group, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            signals.append(observed)
            return real_killpg(group, sig)

        with tempfile.TemporaryDirectory(prefix="cedar-java-supervisor-") as directory:
            root = Path(directory)
            helper = root / "helper.py"
            helper.write_text(source, encoding="utf-8")
            log = root / "private.log"
            started = time.monotonic()
            error = None
            with mock.patch.object(acceptance.subprocess, "Popen", side_effect=spawn), \
                    mock.patch.object(acceptance.os, "killpg", side_effect=terminate), \
                    mock.patch.object(acceptance, "MAX_LOG_BYTES", cap):
                try:
                    acceptance.bounded_process([sys.executable, str(helper)], root, os.environ.copy(), log, timeout)
                except ValueError as caught:
                    error = str(caught)
            elapsed = time.monotonic() - started
            self.assertEqual(len(processes), 1)
            self.assertIsNotNone(processes[0].returncode)
            # The supervisor consumed its child's one wait status, without an
            # old-PID retry. This makes no claim about escaped descendants.
            with self.assertRaises(ChildProcessError):
                os.waitpid(processes[0].pid, os.WNOHANG)
            data = log.read_bytes()
            self.assertLessEqual(len(data), cap)
            self.assertLess(elapsed, timeout + 3)
            return data, error, signals

    def test_normal_output_reaches_eof_and_direct_child_is_reaped(self):
        data, error, signals = self.run_helper("print('synthetic complete', flush=True)\n", 5, 4096)
        self.assertEqual(data, b"synthetic complete\n")
        self.assertIsNone(error)
        self.assertEqual(signals, [])

    def test_flood_never_overflows_sink_and_original_root_is_reaped(self):
        data, error, signals = self.run_helper(
            "import sys\nsys.stdout.buffer.write(b'x' * 262144)\nsys.stdout.buffer.flush()\n", 5, 4096)
        self.assertLessEqual(len(data), 4096)
        self.assertIn("output exceeded bound", error)
        self.assertEqual(len(signals), 1)

    def test_nonzero_exit_is_not_success_and_owned_wait_status_is_reaped(self):
        _, error, signals = self.run_helper("raise SystemExit(7)\n", 5, 4096)
        self.assertIn("subprocess failed", error)
        self.assertEqual(len(signals), 1)
        self.assertEqual(signals[0].si_code, os.CLD_EXITED)
        self.assertEqual(signals[0].si_status, 7)

    def test_no_explicit_signal_or_wait_after_lost_ownership(self):
        process = mock.Mock()
        process.pid = 123
        process.stdout.fileno.return_value = 42
        selector = mock.MagicMock()
        with tempfile.TemporaryDirectory() as directory, \
                mock.patch.object(acceptance.subprocess, "Popen", return_value=process), \
                mock.patch.object(acceptance.os, "set_blocking"), \
                mock.patch.object(acceptance.os, "waitid", side_effect=ChildProcessError), \
                mock.patch.object(acceptance.os, "killpg") as kill, \
                mock.patch.object(acceptance.selectors, "DefaultSelector", return_value=selector):
            with self.assertRaisesRegex(ValueError, "wait ownership"):
                acceptance.bounded_process(["never launched"], directory, {}, Path(directory) / "private.log", 1)
            kill.assert_not_called()
            process.wait.assert_not_called()
            process.stdout.close.assert_called_once()

    def test_root_exit_does_not_finish_while_inherited_writer_holds_pipe(self):
        source = ("import os, time\nchild = os.fork()\n"
                  "if child == 0:\n    time.sleep(60)\n    os._exit(0)\n"
                  "print('synthetic inherited writer', flush=True)\nos._exit(0)\n")
        data, error, signals = self.run_helper(source, 0.5, 4096)
        self.assertIn(b"synthetic inherited writer", data)
        self.assertIn("wall deadline", error)
        self.assertEqual(len(signals), 1)
        self.assertIsNotNone(signals[0])
        self.assertEqual(signals[0].si_code, os.CLD_EXITED)
        self.assertEqual(signals[0].si_status, 0)


class SiblingHarnessTests(unittest.TestCase):
    def test_owned_copies_keep_exact_agent_and_harness_bytes(self):
        import hashlib
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(strict=True)
            source = root / "source"
            source.mkdir()
            harness = source / "test-harness"
            agent = source / "normal-agent"
            harness.write_bytes(b"synthetic test input, never executed")
            agent.write_bytes(b"synthetic agent input, never executed")
            harness.chmod(0o700)
            agent.chmod(0o700)
            scratch = root / "owned scratch"
            scratch.mkdir()
            copied, sibling = acceptance.prepare_sibling_harness(scratch, harness, agent, hashlib.sha256(agent.read_bytes()).digest())
            self.assertEqual(copied.parent, scratch / "Local bundle 雪")
            self.assertEqual(sibling, copied.parent / "cedar-agent")
            self.assertEqual(copied.read_bytes(), harness.read_bytes())
            self.assertEqual(sibling.read_bytes(), agent.read_bytes())
            self.assertTrue(os.access(copied, os.X_OK))
            self.assertTrue(os.access(sibling, os.X_OK))
            with self.assertRaises(FileExistsError):
                acceptance.prepare_sibling_harness(scratch, harness, agent, hashlib.sha256(agent.read_bytes()).digest())

    def test_wrong_agent_identity_is_rejected_before_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(strict=True)
            executable = root / "synthetic-input"
            executable.write_bytes(b"never executed")
            executable.chmod(0o700)
            scratch = root / "scratch"
            scratch.mkdir()
            with self.assertRaisesRegex(ValueError, "identity mismatch"):
                acceptance.prepare_sibling_harness(scratch, executable, executable, bytes(32))


if __name__ == "__main__":
    unittest.main()
