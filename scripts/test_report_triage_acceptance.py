#!/usr/bin/env python3
"""Pure strict-contract and driver tests; never compile or execute Rust."""
from contextlib import redirect_stdout
import copy
import io
import json
import re
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import report_triage_acceptance as proof


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

    def test_fixed_ten_request_ledger_and_fifteen_cases(self):
        expected = proof.expected_receipt("linux")
        self.assertEqual(expected["cases"], [
            'explicit_trust_off_report_read',
            'filtered_xml_order_duplicate_identity',
            'arrow_boundary_clamp',
            'home_end_reveal',
            'enter_details_back_exact_row',
            'space_details_back_exact_row',
            'name_filter_empty_no_retarget',
            'stale_filter_capture_rejected',
            'ready_read_capture_rejected',
            'newer_load_rejects_late_read',
            'stale_session_capture_rejected',
            'fresh_reconnect_navigation',
            'dirty_full_selection_native_undo_redo',
            'automatic_height_reset_preserves_report_and_draft',
            'exact_ledger_disk_and_owner_cleanup',
        ])
        self.assertEqual(len(set(expected["cases"])), 15)
        self.assertEqual(expected["cases_executed"], 15)
        self.assertIs(expected["automatic_height_reset_preserved_state"], True)
        self.assertEqual(len(expected["operation_ledger"]), 10)
        self.assertEqual([item["op"] for item in expected["operation_ledger"]],
                         ["Hello", "List", "Read", "Read", "Read", "Read", "Read", "Hello", "List", "Read"])
        self.assertEqual([item["phase"] for item in expected["operation_ledger"]],
                         ["setup"] * 4 + ["refresh"] * 3 + ["reconnect"] * 3)
        self.assertEqual(len(expected), 55)
        self.assertEqual(expected["source_version"], "0.63.2")
        self.assertEqual((expected["requests"], expected["connections"], expected["agents_reaped"]),
                         (10, 2, 2))
        self.assertEqual((expected["setup_requests"], expected["refresh_requests"],
                          expected["reconnect_requests"], expected["navigation_operations"]),
                         (4, 3, 3, 0))
        self.assertEqual((expected["shared_watchdog_seconds"], expected["total_budget_seconds"],
                          expected["request_admission_seconds"], expected["cleanup_timeout_seconds"]),
                         (60, 65, 30, 5))
        self.assertEqual((expected["report_rows"], expected["filtered_rows"],
                          expected["first_filtered_index"], expected["last_filtered_index"]),
                         (192, 64, 1, 190))
        self.assertTrue(all(set(item) == {"source", "sha256"}
                            for item in expected["source_hashes"]))
        self.assertNotIn("path", json.dumps(expected))
        self.assertNotIn("duplicate café", json.dumps(expected, ensure_ascii=False))
        self.assertEqual(expected["source_hashes"], [
            {"source": "draft", "sha256": "5383e74c2ab999d8eae0978d725ca7eba35344ae9f370c74c688679dcf3b3db1"},
            {"source": "report", "sha256": "bc24e7e96e0badbbd9bebce5e5a0da12062fdd9880c0074855bdd213d1ef09e9"},
        ])

    def test_old_receipt_and_unexecuted_or_extra_reset_case_are_rejected(self):
        for platform in ("linux", "windows"):
            expected = proof.expected_receipt(platform)
            legacy = copy.deepcopy(expected)
            legacy["cases"].remove("automatic_height_reset_preserves_report_and_draft")
            legacy["cases_executed"] = 14
            del legacy["automatic_height_reset_preserved_state"]
            for changes in (
                legacy,
                {**expected, "cases_executed": 14},
                {**expected, "cases_executed": 16},
                {**expected, "automatic_height_reset_preserved_state": False},
                {**expected, "automatic_height_reset_preserved_state": 1},
                {**expected, "cases": expected["cases"][:13] + expected["cases"][14:]},
                {**expected, "cases": expected["cases"] + [expected["cases"][13]]},
            ):
                with self.subTest(platform=platform, receipt=changes), self.assertRaises(proof.AcceptanceError):
                    proof.validate_execution(payload(changes, platform), platform)

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
            good.replace(proof.PREFIX.encode(), b"Report_triage_acceptance "),
            good.replace(proof.PREFIX.encode(), b"report_triage_acceptance\t"),
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

    def test_runtime_provenance_requires_the_selected_test_name(self):
        good = payload()
        for bad in (
            good.replace(("test " + proof.TEST_NAME + " ... ").encode(), b""),
            good.replace(proof.TEST_NAME.encode(), b"unrelated_test"),
            good + ("test " + proof.TEST_NAME + " ... ok\n").encode(),
            good.replace(("test " + proof.TEST_NAME + " ... ").encode(),
                         ("test " + proof.TEST_NAME + " ... FAILED ").encode()),
        ):
            with self.subTest(data=bad[:70]), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(bad, "linux")
        # libtest may print the receipt separately from its named success line.
        standalone = (proof.PREFIX + json.dumps(proof.expected_receipt("linux")) + "\n"
                      + "running 1 test\n" + "test " + proof.TEST_NAME + " ... ok\n"
                      + "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
                        "1900 filtered out; finished in 0.02s\n").encode()
        proof.validate_execution(standalone, "linux")

    def test_driver_name_exists_as_the_single_ignored_source_test(self):
        root = Path(__file__).resolve().parent.parent
        source = (root / "crates/app/src/report_triage_process_tests.rs").read_text(encoding="utf-8")
        tests = re.findall(r'#\[test\]\s*#\[ignore = "[^"\n]+"\]\s*fn (\w+)\(', source)
        self.assertEqual(["report_triage_process_tests::" + name for name in tests],
                         [proof.TEST_NAME])
        self.assertEqual(source.count("#[test]"), 1)
        case_block = source[source.index("const CASES"):source.index("type Check")]
        self.assertEqual(re.findall(r'    "([a-z_]+)",', case_block), list(proof.CASES))
        self.assertIn("const CASES: [&str; 15]", source)
        self.assertEqual(re.findall(r'h\.case\(CASES\[(\d+)\]\)\?', source),
                         [str(index) for index in range(15)])
        self.assertIn('"cases_executed": 15', source)
        self.assertIn('"automatic_height_reset_preserved_state",', source)
        self.assertIn('require(fields.len() == 55, "receipt field count differed")', source)
        self.assertIn("CEDAR_REPORT_TRIAGE_AGENT_BIN", source)
        self.assertIn('eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);', source)
        self.assertIn('mod report_triage_process_tests;',
                      (root / "crates/app/src/lib.rs").read_text(encoding="utf-8"))


class ArtifactProvenanceTests(unittest.TestCase):
    def test_rejects_wrong_source_binary_duplicate_artifact_or_non_test(self):
        with tempfile.TemporaryDirectory(prefix="triage-artifact-") as directory:
            root = Path(directory).resolve()
            deps = root / "target/debug/deps"
            deps.mkdir(parents=True)
            executable = deps / "cedar_app-ab12"
            executable.write_bytes(b"synthetic, never executed")
            good = artifact(root, executable)
            self.assertEqual(proof.compiled_test(json.dumps(good).encode(), root, "linux"), executable)
            values = []
            for mutate in (
                lambda value: value.update(manifest_path=str(root / "other/Cargo.toml")),
                lambda value: value["target"].update(src_path=str(root / "other/lib.rs")),
                lambda value: value["target"].update(kind=["bin"]),
                lambda value: value["profile"].update(test=False),
                lambda value: value["profile"].update(test=1),
                lambda value: value.update(executable=str(root / "target/release/cedar_app-ab12")),
                lambda value: value.update(executable="target/debug/deps/cedar_app-ab12"),
                lambda value: value.update(executable=str(deps / "cedar_app-ab12.exe")),
                lambda value: value.update(executable=str(deps / "cedar_app-missing")),
            ):
                value = copy.deepcopy(good)
                mutate(value)
                values.append(json.dumps(value).encode())
            encoded = json.dumps(good).encode()
            values.extend((encoded + b"\n" + encoded,
                           encoded.replace(b'"test": true', b'"test": true, "test": true')))
            for value in values:
                with self.subTest(value=value[:80]), self.assertRaises(proof.AcceptanceError):
                    proof.compiled_test(value, root, "linux")


class SourceAndAgentProvenanceTests(unittest.TestCase):
    def test_manifest_requires_exact_source_version_and_bounded_valid_toml(self):
        with tempfile.TemporaryDirectory(prefix="triage-source-") as directory:
            root = Path(directory).resolve()
            path = root / "Cargo.toml"
            with self.assertRaises(proof.AcceptanceError):
                proof.validate_source_version(root)
            for invalid in (b"", b"\xff", b"[", b'workspace = "wrong"',
                            b'[workspace]\npackage = false',
                            b'[workspace.package]\nversion = 54',
                            b'[workspace.package]\nversion = "0.53.4"',
                            b'[workspace.package]\nversion = "0.63.2-extra"',
                            b'[workspace.package]\nversion = "0.63.2"\nversion = "0.63.2"',
                            b" " * (64 * 1024 + 1)):
                path.write_bytes(invalid)
                with self.subTest(data=invalid[:60]), self.assertRaises(proof.AcceptanceError):
                    proof.validate_source_version(root)
            path.write_text('[workspace.package]\nversion = "0.63.2"\n')
            proof.validate_source_version(root)

    def test_agent_digest_is_exact_nonempty_bounded_and_detects_same_size_change(self):
        with tempfile.TemporaryDirectory(prefix="triage-agent-") as directory:
            path = Path(directory).resolve() / "cedar-agent"
            with self.assertRaises(proof.AcceptanceError):
                proof.agent_digest(path)
            path.write_bytes(b"")
            with self.assertRaises(proof.AcceptanceError):
                proof.agent_digest(path)
            path.write_bytes(b"first")
            before = proof.agent_digest(path)
            self.assertEqual(before[0], 5)
            path.write_bytes(b"other")
            self.assertNotEqual(proof.agent_digest(path), before)
            with mock.patch.object(proof, "MAX_AGENT_BYTES", 4), self.assertRaises(proof.AcceptanceError):
                proof.agent_digest(path)

    def test_agent_symlink_cannot_substitute_for_selected_release_binary(self):
        with tempfile.TemporaryDirectory(prefix="triage-agent-link-") as directory:
            root = Path(directory).resolve()
            target, link = root / "other", root / "cedar-agent"
            target.write_bytes(b"synthetic, never executed")
            try:
                link.symlink_to(target)
            except OSError:
                self.skipTest("symlink creation unavailable on this test host")
            with self.assertRaises(proof.AcceptanceError):
                proof.agent_digest(link)


class DriverTests(unittest.TestCase):
    def exercise(self, platform="linux", *, failure=None, selection=None, runtime=None,
                 missing_agent=False, invalid_artifact=False, source_version="0.63.2",
                 changed_agent=False, changed_source=False):
        with tempfile.TemporaryDirectory(prefix="cedar-triage-lines-driver-") as directory:
            root = Path(directory).resolve()
            deps, release = root / "target/debug/deps", root / "target/release"
            deps.mkdir(parents=True)
            release.mkdir()
            manifest = root / "Cargo.toml"
            manifest.write_text('[workspace.package]\nversion = ' + json.dumps(source_version) + '\n')
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
                self.assertEqual(environment["CEDAR_REPORT_TRIAGE_AGENT_BIN"], str(agent))
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
                    if changed_agent:
                        agent.write_bytes(b"different synthetic binary, never executed")
                    if changed_source:
                        manifest.write_text('[workspace.package]\nversion = "0.53.4"\n')
                if stage == failure:
                    raise proof.AcceptanceError("subprocess_timeout")
                return data

            capture = io.StringIO()
            with mock.patch.object(proof.sys, "platform", "win32" if platform == "windows" else platform), \
                    mock.patch.object(proof, "bounded_process", side_effect=execute), \
                    mock.patch.dict(proof.os.environ, {"CEDAR_REPORT_TRIAGE_AGENT_BIN": "wrong",
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
                self.assertIn("15 cases, 10 explicit requests, 0 navigation requests, 2 agents reaped", output)

    def test_preparation_failure_never_starts_runtime(self):
        for changes, expected_calls in (({"missing_agent": True}, 0), ({"source_version": "0.53.4"}, 0),
                                        ({"failure": "compile"}, 1),
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
                        {"runtime": payload(bad)}, {"runtime": payload(platform="windows")},
                        {"changed_agent": True}, {"changed_source": True}):
            with self.subTest(changes=changes):
                receipt, error, calls, output = self.exercise(**changes)
                self.assertIsNone(receipt)
                self.assertIsNotNone(error)
                self.assertEqual(len(calls), 3)
                self.assertEqual(output, "")


class DiagnosticSupervisorTests(unittest.TestCase):
    def exercise(self, chunks, *, returncode=0, timeout=10, limit=64,
                 cleanup_error=None, close_error=None):
        import report_triage_diagnostics as diagnostic
        process = mock.Mock()
        process.stdout.fileno.return_value = 42
        process.stdout.close.side_effect = close_error
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

        with mock.patch.object(diagnostic.subprocess, "Popen", return_value=process) as launch, \
                mock.patch.object(diagnostic.os, "set_blocking"), \
                mock.patch.object(diagnostic.os, "read", side_effect=read), \
                mock.patch.object(diagnostic.time, "monotonic", side_effect=now), \
                mock.patch.object(diagnostic.time, "sleep"):
            output, error = None, None
            try:
                output = diagnostic.bounded_process(["synthetic, never launched"], Path("."), {}, timeout, limit)
            except diagnostic.CapturedFailure as caught:
                error = caught
        launch.assert_called_once()
        process.stdout.close.assert_called_once()
        process.wait.assert_called_once_with(timeout=5)
        return output, error, process

    def test_complete_output_retains_identical_success_contract(self):
        output, error, process = self.exercise([BlockingIOError(), b"one\n", b"two\n", b""])
        self.assertEqual(output, b"one\ntwo\n")
        self.assertIsNone(error)
        process.kill.assert_not_called()

    def test_nonzero_retains_bounded_merged_output_and_eof_reap_metadata(self):
        output, error, process = self.exercise([b"stdout\nstderr\n", b""], returncode=7)
        self.assertIsNone(output)
        self.assertEqual(str(error), "subprocess_nonzero")
        self.assertEqual(error.output, b"stdout\nstderr\n")
        self.assertEqual((error.returncode, error.eof, error.cleanup, error.truncated),
                         (7, True, "reaped", False))
        self.assertNotIn("stdout", str(error))
        process.kill.assert_not_called()

    def test_cap_is_exact_and_overflow_prefix_is_bounded(self):
        output, error, _ = self.exercise([b"12345678", b""], limit=8)
        self.assertEqual(output, b"12345678")
        self.assertIsNone(error)
        output, error, process = self.exercise([b"1234567", b"89"], limit=8, returncode=None)
        self.assertIsNone(output)
        self.assertEqual(str(error), "subprocess_output_limit")
        self.assertEqual(error.output, b"12345678")
        self.assertEqual((error.returncode, error.eof, error.cleanup, error.truncated),
                         (-9, False, "reaped", True))
        process.kill.assert_called_once()

    def test_timeout_covers_eof_after_exit_and_capture_is_incomplete(self):
        import report_triage_diagnostics as diagnostic
        for code in (None, 0):
            _, error, process = self.exercise([b"partial"], returncode=code, timeout=0.1)
            self.assertEqual(str(error), "subprocess_timeout")
            self.assertEqual(error.output, b"partial")
            self.assertFalse(error.eof)
            metadata = diagnostic.failure_artifact(diagnostic.StageFailure(
                "runtime", error, Path(__file__).resolve().parent.parent, 90, 256 * 1024))
            self.assertFalse(metadata["capture_complete"])
            self.assertEqual(process.kill.call_count, int(code is None))

    def test_failed_reap_overrides_failure_without_losing_output(self):
        import subprocess
        _, error, _ = self.exercise([b"private input", b""], returncode=7,
                                   cleanup_error=subprocess.TimeoutExpired("never echoed", 5))
        self.assertEqual(str(error), "child_cleanup_unverified")
        self.assertEqual(error.output, b"private input")
        self.assertEqual(error.cleanup, "unverified")
        self.assertTrue(error.eof)

    def test_close_and_read_errors_keep_capture_and_real_reap_status(self):
        for changes in ({"close_error": OSError("private detail")}, {}):
            chunks = [b"partial", b""] if changes else [b"partial", OSError("private detail")]
            _, error, _ = self.exercise(chunks, **changes)
            self.assertEqual(str(error), "subprocess_io_error")
            self.assertEqual(error.output, b"partial")
            self.assertEqual(error.cleanup, "reaped")
            self.assertNotIn("private", str(error))

    def test_launch_failure_is_not_started_and_never_leaks_exception(self):
        import report_triage_diagnostics as diagnostic
        with mock.patch.object(diagnostic.subprocess, "Popen", side_effect=OSError("secret /private/path")):
            with self.assertRaises(diagnostic.CapturedFailure) as caught:
                diagnostic.bounded_process(["never launched"], Path("."), {}, 90, 256 * 1024)
        self.assertEqual(str(caught.exception), "subprocess_launch_error")
        self.assertEqual(caught.exception.cleanup, "not_started")
        self.assertEqual(caught.exception.output, b"")
        self.assertIsNone(caught.exception.returncode)


class DiagnosticPrivacyTests(unittest.TestCase):
    def test_each_stage_nonzero_is_named_once_without_retry_or_raw_console(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        for stage, timeout, limit in (("compile", 120, 4 * 1024 * 1024),
                                      ("selection", 15, 16 * 1024),
                                      ("runtime", 90, 256 * 1024)):
            failure = diagnostic.CapturedFailure("subprocess_nonzero", b"SECRET fixture /private/file",
                                                 returncode=9, eof=True, cleanup="reaped")
            runner = mock.Mock(side_effect=failure)
            with self.assertRaises(diagnostic.StageFailure) as caught:
                diagnostic.run_stage(stage, root, ["never launched"], {}, timeout, limit,
                                     mock.Mock(), runner)
            runner.assert_called_once()
            self.assertEqual(str(caught.exception), stage + ":subprocess_nonzero")
            item = diagnostic.failure_artifact(caught.exception)
            self.assertEqual((item["stage"], item["returncode"], item["capture_limit_bytes"],
                              item["timeout_seconds"], item["native_cleanup"]),
                             (stage, 9, limit, timeout, "unverified"))
            self.assertTrue(item["capture_complete"])
            self.assertNotIn("SECRET", json.dumps(item))
            self.assertNotIn("/private", json.dumps(item))

    def test_allowlisted_native_check_retains_only_reviewed_message_and_location(self):
        import hashlib
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        data = (b'Error: "expected rendered label absent"\n'
                b'Error: "SECRET fixture content"\nprivate password=SECRET /home/private/file\n')
        failure = diagnostic.CapturedFailure("subprocess_nonzero", data, returncode=1, eof=True, cleanup="reaped")
        item = diagnostic.failure_artifact(diagnostic.StageFailure("runtime", failure, root, 90, 256 * 1024))
        self.assertEqual(item["diagnostics"], [{"kind": "native_check", "message": "expected rendered label absent",
                                              "source": "crates/app/src/report_triage_process_tests.rs",
                                              "line": diagnostic.native_errors(root)["expected rendered label absent"][0]}])
        self.assertEqual(item["raw_output_omitted_bytes"], len(data))
        self.assertEqual(item["raw_output_omitted_sha256"], hashlib.sha256(data).hexdigest())
        safe = json.dumps(item["diagnostics"], ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode()
        self.assertEqual(item["sanitized_diagnostic_bytes"], len(safe))
        self.assertEqual(item["suppressed_lines"], 2)
        for secret in ("SECRET", "password", "/home/private"):
            self.assertNotIn(secret, json.dumps(item))

    def test_compiler_prose_source_and_private_spans_are_omitted(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        source = root / "crates/app/src/lib.rs"
        value = {"reason": "compiler-message", "message": {
            "level": "error", "code": {"code": "E0308"}, "message": "SECRET document text",
            "rendered": "private token=SECRET", "spans": [
                {"file_name": str(source), "line_start": 7, "column_start": 3,
                 "text": [{"text": "private source SECRET"}]},
                {"file_name": "/home/private/secret.rs", "line_start": 1, "column_start": 1},
                {"file_name": "../crates/app/src/lib.rs", "line_start": 1, "column_start": 1},
            ]}}
        items, _, _ = diagnostic.sanitize_output(json.dumps(value).encode(), root, "compile")
        self.assertEqual(items, [{"kind": "compiler_error", "code": "E0308", "locations": [
            {"source": "crates/app/src/lib.rs", "line": 7, "column": 3}]}])
        self.assertNotIn("SECRET", json.dumps(items))
        self.assertNotIn(str(root), json.dumps(items))
        value["message"]["code"]["code"] = "E0308 SECRET"
        self.assertEqual(diagnostic.sanitize_output(json.dumps(value).encode(), root, "compile")[0], [])

    def test_native_divider_checks_match_source_and_omit_unknown_prose_and_paths(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        messages = (
            "Tools panel did not retain the native 480-point resize",
            "initial Tools resize must precede the explicit report Read",
            "native Tools resize handle absent",
            "native Tools resize handle did not own the drag",
            "native Tools resize handle did not receive the primary press",
            "native Tools resize handle did not receive the primary release",
            "native Tools resize handle invalid",
            "rendered Tools panel geometry invalid",
            "rendered Tools panel state absent",
        )
        source = (root / "crates/app/src/report_triage_process_tests.rs").read_text().splitlines()
        known = diagnostic.native_errors(root)
        for message in messages:
            self.assertIn(message, diagnostic.NATIVE_ERRORS)
            self.assertEqual(known[message], [line for line, text in enumerate(source, 1)
                                              if '"' + message + '"' in text])
            self.assertEqual(len(known[message]), 1)
        data = b"".join(f'Error: "{message}"\n'.encode() for message in messages)
        data += (b'Error: "native Tools resize handle invalid SECRET"\n'
                 b'Error: "SECRET fixture content"\n/private/SECRET report-body=SECRET\n')
        failure = diagnostic.CapturedFailure("subprocess_nonzero", data, returncode=1, eof=True, cleanup="reaped")
        item = diagnostic.failure_artifact(diagnostic.StageFailure("runtime", failure, root, 90, 256 * 1024))
        self.assertEqual(item["diagnostics"], [
            {"kind": "native_check", "message": message,
             "source": "crates/app/src/report_triage_process_tests.rs", "line": known[message][0]}
            for message in messages])
        self.assertEqual(item["suppressed_lines"], 3)
        self.assertFalse(item["diagnostics_truncated"])
        self.assertEqual(item["raw_output_omitted_bytes"], len(data))
        self.assertEqual(item["native_cleanup"], "unverified")
        self.assertLess(len(json.dumps(item).encode()), diagnostic.MAX_ARTIFACT_BYTES)
        for private in ("SECRET", "/private", "report-body", str(root)):
            self.assertNotIn(private, json.dumps(item))

    def test_automatic_height_checks_are_fixed_source_literals_without_private_payloads(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        messages = (
            "Automatic height control did not clear the manual preference",
            "Automatic height control was not enabled",
            "Automatic height preference was restored after settling",
            "Automatic height reset changed the operation ledger",
            "Automatic height reset changed the report snapshot or selection",
            "Automatic height reset did not start with a manual report height",
            "Automatic height reset lost native Redo state",
            "Automatic height reset lost native Undo selection",
        )
        source = (root / "crates/app/src/report_triage_process_tests.rs").read_text().splitlines()
        known = diagnostic.native_errors(root)
        for message in messages:
            self.assertIn(message, diagnostic.NATIVE_ERRORS)
            self.assertEqual(known[message], [line for line, text in enumerate(source, 1)
                                              if '"' + message + '"' in text])
            self.assertEqual(len(known[message]), 1)
        data = b"".join(f'Error: "{message}"\n'.encode() for message in messages)
        data += (b'Error: "Automatic height reset changed the operation ledger SECRET"\n'
                 b'Error: "SECRET fixture content"\n/private/SECRET report-body=SECRET\n')
        failure = diagnostic.CapturedFailure("subprocess_nonzero", data, returncode=1, eof=True, cleanup="reaped")
        item = diagnostic.failure_artifact(diagnostic.StageFailure("runtime", failure, root, 90, 256 * 1024))
        self.assertEqual(item["diagnostics"], [
            {"kind": "native_check", "message": message,
             "source": "crates/app/src/report_triage_process_tests.rs", "line": known[message][0]}
            for message in messages])
        self.assertEqual(item["suppressed_lines"], 3)
        self.assertFalse(item["diagnostics_truncated"])
        self.assertEqual(item["raw_output_omitted_bytes"], len(data))
        self.assertEqual(item["native_cleanup"], "unverified")
        for private in ("SECRET", "/private", "report-body", str(root)):
            self.assertNotIn(private, json.dumps(item))

    def test_panic_keeps_only_verified_relative_location(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        data = (f"thread 'SECRET' panicked at {root}/crates/app/src/lib.rs:9:4:\n"
                "assertion failed: SECRET /home/private\n").encode()
        self.assertEqual(diagnostic.sanitize_output(data, root, "runtime")[0], [
            {"kind": "panic_location", "source": "crates/app/src/lib.rs", "line": 9, "column": 4}])

    def test_location_rejects_missing_nonrust_traversal_symlink_and_invalid_numbers(self):
        import report_triage_diagnostics as diagnostic
        with tempfile.TemporaryDirectory(prefix="triage-diagnostic-source-") as directory:
            root = Path(directory).resolve()
            source = root / "crates/app/src/lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text("synthetic source, never compiled")
            for name, line, column in (("crates/app/src/missing.rs", 1, 1),
                                       ("crates/app/src/lib.txt", 1, 1),
                                       ("crates/app/src/../src/lib.rs", 1, 1),
                                       ("crates/app/src/lib.rs", True, 1),
                                       ("crates/app/src/lib.rs", 0, 1),
                                       ("crates/app/src/lib.rs", 1, 100001)):
                self.assertIsNone(diagnostic.source_location(root, name, line, column))
            link = source.parent / "link.rs"
            try:
                link.symlink_to(source)
            except OSError:
                return
            self.assertIsNone(diagnostic.source_location(root, str(link), 1, 1))

    def test_diagnostic_count_and_artifact_bytes_are_bounded(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        data = b"\n".join(f'Error: "{message}"'.encode() for message in sorted(diagnostic.NATIVE_ERRORS))
        failure = diagnostic.CapturedFailure("subprocess_nonzero", data, returncode=1, eof=True, cleanup="reaped")
        error = diagnostic.StageFailure("runtime", failure, root, 90, 256 * 1024)
        item = diagnostic.failure_artifact(error)
        self.assertEqual(len(item["diagnostics"]), 32)
        self.assertTrue(item["diagnostics_truncated"])
        self.assertLess(len(json.dumps(item).encode()), diagnostic.MAX_ARTIFACT_BYTES)

    def test_artifact_exclusive_creation_disabled_default_and_no_raw_dump(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        error = diagnostic.StageFailure("runtime", diagnostic.CapturedFailure(
            "subprocess_nonzero", b"SECRET raw body", returncode=1, eof=True, cleanup="reaped"), root, 90, 256 * 1024)
        self.assertEqual(diagnostic.write_failure_artifact(error, None), "disabled")
        self.assertEqual(diagnostic.write_failure_artifact(error, "relative"), "unavailable")
        with tempfile.TemporaryDirectory(prefix="triage-safe-diagnostic-") as directory:
            folder = Path(directory).resolve() / "evidence"
            self.assertEqual(diagnostic.write_failure_artifact(error, str(folder)), "written")
            self.assertEqual([path.name for path in folder.iterdir()], [diagnostic.ARTIFACT_NAME])
            before = (folder / diagnostic.ARTIFACT_NAME).read_bytes()
            self.assertNotIn(b"SECRET", before)
            self.assertEqual(diagnostic.write_failure_artifact(error, str(folder)), "unavailable")
            self.assertEqual((folder / diagnostic.ARTIFACT_NAME).read_bytes(), before)

    def test_actual_compiler_validation_categories_stay_fixed(self):
        import report_triage_diagnostics as diagnostic
        for category in ("invalid_test_artifact", "unexpected_test_source", "unexpected_test_binary"):
            self.assertEqual(str(diagnostic.CapturedFailure(category)), category)
        self.assertEqual(str(diagnostic.CapturedFailure("SECRET private error")), "unclassified_failure")


class DiagnosticEdgeTests(unittest.TestCase):
    def test_exact_native_test_prefix_and_other_prefixes(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        self.assertEqual(diagnostic.TEST_NAME, proof.TEST_NAME)
        message = b'Error: "expected rendered label absent"'
        expected = diagnostic.sanitize_output(message, root, "runtime")[0]
        self.assertTrue(expected)
        prefixed = ("test " + proof.TEST_NAME + " ... ").encode() + message
        self.assertEqual(diagnostic.sanitize_output(prefixed, root, "runtime")[0], expected)
        for bad in (b"test other ... " + message, b"private-prefix " + message,
                    b"Error: \"expected rendered label absent SECRET\""):
            self.assertEqual(diagnostic.sanitize_output(bad, root, "runtime")[0], [])

    def test_validator_exceptions_retain_stage_output_and_known_process_status(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        for error, category in ((OSError("SECRET"), "output_validation_io_error"),
                                (ValueError("SECRET"), "output_validation_error"),
                                (RecursionError("SECRET"), "output_validation_error")):
            runner = mock.Mock(return_value=b"private output")
            with self.assertRaises(diagnostic.StageFailure) as caught:
                diagnostic.run_stage("compile", root, ["never launched"], {}, 120, 4 * 1024 * 1024,
                                     mock.Mock(side_effect=error), runner)
            self.assertEqual(str(caught.exception), "compile:" + category)
            item = diagnostic.failure_artifact(caught.exception)
            self.assertEqual((item["returncode"], item["eof_observed"], item["direct_child_cleanup"]),
                             (0, True, "reaped"))
            self.assertNotIn("SECRET", json.dumps(item))
            self.assertNotIn("private output", json.dumps(item))

    def test_location_and_line_caps_report_diagnostic_truncation(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        for count in (5, 33):
            value = {"reason": "compiler-message", "message": {
                "level": "error", "code": {"code": "E0308"}, "spans": [
                    {"file_name": "crates/app/src/lib.rs", "line_start": index + 1, "column_start": 1}
                    for index in range(count)]}}
            items, suppressed, clipped = diagnostic.sanitize_output(json.dumps(value).encode(), root, "compile")
            self.assertEqual(len(items[0]["locations"]), 4)
            self.assertTrue(clipped)
            self.assertEqual(suppressed, 0)
        self.assertEqual(diagnostic.sanitize_output(b"x" * 65537, root, "runtime"), ([], 1, True))

    def test_native_whitelist_is_current_source_only_and_never_source_derived_prose(self):
        import report_triage_diagnostics as diagnostic
        with tempfile.TemporaryDirectory(prefix="triage-diagnostic-native-") as directory:
            root = Path(directory).resolve()
            source = root / "crates/app/src/report_triage_process_tests.rs"
            source.parent.mkdir(parents=True)
            source.write_text('"SECRET source prose"\n"expected rendered label absent"\n')
            known = diagnostic.native_errors(root)
            self.assertEqual(known, {"expected rendered label absent": [2]})
            source.write_bytes(b"x" * (256 * 1024 + 1))
            self.assertEqual(diagnostic.native_errors(root), {})

    def test_artifact_folder_symlink_is_refused_without_writing(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        error = diagnostic.StageFailure("runtime", diagnostic.CapturedFailure("subprocess_nonzero"),
                                        root, 90, 256 * 1024)
        with tempfile.TemporaryDirectory(prefix="triage-diagnostic-link-") as directory:
            base = Path(directory).resolve()
            destination, link = base / "destination", base / "alias"
            destination.mkdir()
            try:
                link.symlink_to(destination, target_is_directory=True)
            except OSError:
                self.skipTest("symlinks unavailable")
            self.assertEqual(diagnostic.write_failure_artifact(error, str(link)), "unavailable")
            self.assertEqual(list(destination.iterdir()), [])

    def test_main_failure_reports_only_fixed_category_and_artifact_status(self):
        from contextlib import redirect_stderr
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        error = diagnostic.StageFailure("runtime", diagnostic.CapturedFailure(
            "subprocess_nonzero", b'Error: "expected rendered label absent"\nSECRET private document',
            returncode=1, eof=True, cleanup="reaped"), root, 90, 256 * 1024)
        capture = io.StringIO()
        with tempfile.TemporaryDirectory(prefix="triage-diagnostic-main-") as directory:
            folder = Path(directory).resolve() / "evidence"
            with mock.patch.object(proof, "run", side_effect=error), \
                    mock.patch.object(proof.sys, "argv", ["report_triage_acceptance.py"]), \
                    mock.patch.dict(proof.os.environ, {"CEDAR_REPORT_TRIAGE_DIAGNOSTIC_DIR": str(folder)}), \
                    redirect_stderr(capture):
                self.assertEqual(proof.main(), 1)
            self.assertEqual(capture.getvalue(),
                             "Report keyboard triage acceptance failed: runtime:subprocess_nonzero; sanitized_diagnostic=written\n")
            self.assertTrue((folder / diagnostic.ARTIFACT_NAME).is_file())
            self.assertNotIn("SECRET", (folder / diagnostic.ARTIFACT_NAME).read_text())

    def test_ci_upload_is_exact_failure_artifact_for_both_platforms(self):
        root = Path(__file__).resolve().parent.parent
        workflow = (root / ".github/workflows/ci.yml").read_text()
        for platform in ("linux", "windows"):
            self.assertIn("id: report_triage_" + platform, workflow)
            self.assertIn("steps.report_triage_" + platform + ".outcome == 'failure'", workflow)
        self.assertEqual(workflow.count("path: ${{ runner.temp }}/cedar-report-triage-diagnostic/REPORT_TRIAGE_FAILURE.json"), 2)
        self.assertNotIn("cedar-report-triage-diagnostic/*", workflow)


class GeometryDiagnosticTests(unittest.TestCase):
    def context(self, **changes):
        value = {"schema": 1, "case_ordinal": 3, "reason": "target_visibility",
                 "target": "case", "row_index": 190, "phase": "loaded", "geometry_state": "finite",
                 "rect": [320.0, 350.0, 975.0, 368.0],
                 "clip": [300.0, 340.0, 980.0, 780.0], "scroll": [0.0, 2500.0]}
        value.update(changes)
        return value

    def line(self, value):
        import report_triage_diagnostics as diagnostic
        return diagnostic.GEOMETRY_PREFIX + json.dumps(value, ensure_ascii=True, allow_nan=True)

    def test_exact_loaded_geometry_and_native_prefix_survive_sanitization(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        value = self.context()
        expected = {"kind": "report_geometry", **value}
        self.assertEqual(diagnostic.geometry_context(self.line(value)), expected)
        for prefix in ("", "test " + proof.TEST_NAME + " ... "):
            items, suppressed, clipped = diagnostic.sanitize_output(
                (prefix + self.line(value)).encode(), root, "runtime")
            self.assertEqual(items, [expected])
            self.assertEqual(suppressed, 0)
            self.assertFalse(clipped)
        self.assertEqual(diagnostic.sanitize_output(self.line(value).encode(), root, "compile")[0], [])

    def test_fifteen_case_geometry_ordinals_remain_exact_and_bounded(self):
        import report_triage_diagnostics as diagnostic
        for ordinal in range(15):
            value = self.context(case_ordinal=ordinal)
            self.assertEqual(diagnostic.geometry_context(self.line(value)), {"kind": "report_geometry", **value})
        for ordinal in (-1, 15, True, 14.0, "14"):
            self.assertIsNone(diagnostic.geometry_context(self.line(self.context(case_ordinal=ordinal))))

    def test_before_load_is_explicit_and_has_no_row_or_stale_scroll(self):
        import report_triage_diagnostics as diagnostic
        value = self.context(case_ordinal=0, target="load", row_index=None,
                             phase="before_load", scroll=None)
        self.assertEqual(diagnostic.geometry_context(self.line(value)), {"kind": "report_geometry", **value})
        for changes in ({"row_index": 0}, {"scroll": [0, 0]}, {"target": "case"},
                        {"reason": "loaded_viewport"}):
            self.assertIsNone(diagnostic.geometry_context(self.line({**value, **changes})))

    def test_nonfinite_geometry_uses_explicit_invalid_state_with_no_numeric_payload(self):
        import report_triage_diagnostics as diagnostic
        value = self.context(geometry_state="invalid", rect=None, clip=None, scroll=None)
        self.assertEqual(diagnostic.geometry_context(self.line(value)), {"kind": "report_geometry", **value})
        before = {**value, "case_ordinal": 0, "phase": "before_load", "target": "load", "row_index": None}
        self.assertIsNotNone(diagnostic.geometry_context(self.line(before)))
        for key, values in (("rect", [0, 0, 1, 1]), ("clip", [0, 0, 1, 1]), ("scroll", [0, 0])):
            self.assertIsNone(diagnostic.geometry_context(self.line({**value, key: values})))

    def test_rejects_wrong_keys_types_roles_ordinals_indices_and_private_strings(self):
        import report_triage_diagnostics as diagnostic
        bad = [None, [], "SECRET", {**self.context(), "path": "/private/SECRET"},
               {key: value for key, value in self.context().items() if key != "phase"}]
        for key, values in {
            "schema": [True, 1.0, 2, "1"],
            "case_ordinal": [True, -1, 15, 3.0, "3"],
            "row_index": [None, True, -1, 192, 190.0, "190"],
            "target": [[], "SECRET", "case:private", 1],
            "phase": [[], "SECRET", "beforeLoad", True],
            "geometry_state": [[], "NaN", "SECRET", False],
            "reason": [[], "SECRET", "visibility", 1],
            "rect": [None, [], [1, 2, 3], [1, 2, 3, 4, 5], "SECRET"],
            "clip": [None, "SECRET"],
            "scroll": [None, [], [0, 0, 0], "SECRET"],
        }.items():
            bad.extend(self.context(**{key: value}) for value in values)
        bad.extend((self.context(target="load"), self.context(reason="loaded_viewport"),
                    self.context(target="back", row_index=None),
                    self.context(target="details", row_index=None)))
        for value in bad:
            with self.subTest(value=value):
                self.assertIsNone(diagnostic.geometry_context(self.line(value)))

    def test_rejects_boolean_nonfinite_huge_and_out_of_range_coordinates(self):
        import report_triage_diagnostics as diagnostic
        for key, count in (("rect", 4), ("clip", 4), ("scroll", 2)):
            for number in (True, False, "SECRET", None, [], float("nan"), float("inf"),
                           -float("inf"), 100001, -100001, 10 ** 1000):
                values = [0] * count
                values[0] = number
                with self.subTest(key=key, number_type=type(number)):
                    self.assertIsNone(diagnostic.geometry_context(self.line(self.context(**{key: values}))))
        boundary = self.context(rect=[-100000, 0, 100000, 1], scroll=[100000, -100000])
        self.assertIsNotNone(diagnostic.geometry_context(self.line(boundary)))

    def test_duplicate_keys_malformed_and_oversized_context_are_rejected(self):
        import report_triage_diagnostics as diagnostic
        good = self.line(self.context())
        for bad in (good.replace('"schema": 1', '"schema": 1, "schema": 1'),
                    good.replace("320.0", "1e309", 1),
                    good[:-1], good + "SECRET", good.replace(diagnostic.GEOMETRY_PREFIX, "wrong ", 1),
                    diagnostic.GEOMETRY_PREFIX + " " * diagnostic.MAX_GEOMETRY_BYTES + "{}"):
            self.assertIsNone(diagnostic.geometry_context(bad))
        root = Path(__file__).resolve().parent.parent
        oversized = diagnostic.GEOMETRY_PREFIX + " " * diagnostic.MAX_GEOMETRY_BYTES + "{}"
        self.assertEqual(diagnostic.sanitize_output(oversized.encode(), root, "runtime"), ([], 1, True))

    def test_loaded_viewport_context_is_only_exact_first_row(self):
        import report_triage_diagnostics as diagnostic
        value = self.context(reason="loaded_viewport", row_index=0)
        self.assertIsNotNone(diagnostic.geometry_context(self.line(value)))
        for changes in ({"row_index": 1}, {"row_index": None}, {"target": "back"},
                        {"phase": "before_load"}):
            self.assertIsNone(diagnostic.geometry_context(self.line({**value, **changes})))

    def test_failure_artifact_contains_no_unknown_geometry_payload(self):
        import report_triage_diagnostics as diagnostic
        root = Path(__file__).resolve().parent.parent
        data = (self.line(self.context()) + "\n" + self.line(self.context(rect=["SECRET", 0, 0, 0]))
                + '\nError: "focused report target was not revealed in its real viewport"\n'
                  'fixture document SECRET path=/private/SECRET\n').encode()
        failure = diagnostic.CapturedFailure("subprocess_nonzero", data, returncode=1, eof=True, cleanup="reaped")
        item = diagnostic.failure_artifact(diagnostic.StageFailure("runtime", failure, root, 90, 256 * 1024))
        self.assertEqual([entry["kind"] for entry in item["diagnostics"]], ["report_geometry", "native_check"])
        self.assertEqual(item["suppressed_lines"], 2)
        self.assertNotIn("SECRET", json.dumps(item))
        self.assertNotIn("/private", json.dumps(item))
        self.assertLess(len(json.dumps(item).encode()), diagnostic.MAX_ARTIFACT_BYTES)

    def test_reset_case_uses_real_header_frames_before_unchanged_cleanup(self):
        root = Path(__file__).resolve().parent.parent
        source = (root / "crates/app/src/report_triage_process_tests.rs").read_text()
        start = source.index("    h.case(CASES[12])?;")
        end = source.index("    let expected = serde_json::json!([", start)
        reset = source[start:end]
        ordered = ["h.focus_row(190)?", "h.details(190, false)?", "h.resized_report_panel()?",
                   "h.app.tools_height.is_some_and", "let reset_snapshot", "let reset_report_state",
                   "let reset_buffers = h.buffers()", "let reset_selection = h.selection(draft)?",
                   "let reset_history = h.history(draft)?", "let reset_ledger = h.ledger.ordered.clone()",
                   'h.click_probe("tools_automatic_height", false)?', "h.app.tools_height.is_none()",
                   "h.settle()?", "h.app.tools_height.is_none()",
                   "h.preserved(&reset_buffers, draft, reset_selection, &reset_history)?",
                   "snapshot.source == reset_snapshot.0", "snapshot.revision == reset_snapshot.1",
                   "snapshot.report == reset_snapshot.2", ") == reset_report_state",
                   "h.ledger.ordered == reset_ledger && h.ledger.counts() == reset_counts",
                   "h.undo(false, DRAFT, DRAFT_BASE)?", "h.undo(true, DRAFT, DRAFT_TEXT)?",
                   "h.history(draft)? == reset_history", "h.case(CASES[13])?"]
        cursor = 0
        for part in ordered:
            cursor = reset.index(part, cursor) + len(part)
        for forbidden in ("tools_height =", "reset_tools_height", "insert_persisted", "PanelState {",
                          "Operation::", "load_test_report", "load_report_control", "exchange(",
                          "std::process", "Command::new"):
            self.assertNotIn(forbidden, reset)
        self.assertEqual(source.count('h.click_probe("tools_automatic_height", false)?'), 1)
        self.assertLess(source.index("    h.case(CASES[13])?;"), source.index("    h.reap()?;"))
        self.assertIn("    h.case(CASES[14])?;\n    h.stop_watchdog()?;", source)
        click = source[source.index("    fn click_probe("):source.index("    fn click_label(")]
        self.assertIn("self.visible_probe(name)?", click)
        self.assertIn("self.click_at(at, request)", click)
        pointer = source[source.index("    fn click_at("):source.index("    fn click_probe(")]
        self.assertIn("for pressed in [true, false]", pointer)
        self.assertIn("egui::Event::PointerButton", pointer)
        self.assertIn("button: egui::PointerButton::Primary", pointer)
        self.assertIn("self.frame(events)?", pointer)

    def test_fixture_native_resize_precedes_load_and_survives_read_without_reseeding(self):
        root = Path(__file__).resolve().parent.parent
        source = (root / "crates/app/src/report_triage_process_tests.rs").read_text()
        start = source.index("    fn resize_report_panel(")
        end = source.index("    fn source(", start)
        resize = source[start:end]
        ordered = ['egui::Id::new("tools").with("__resize")', "handle.rect.center()",
                   "self.report_panel_rect()?.bottom() - REPORT_PANEL_HEIGHT", "pressed: true",
                   "response.is_pointer_button_down_on()", "egui::Event::PointerMoved(target)",
                   "response.dragged_by(egui::PointerButton::Primary)", "pressed: false",
                   "response.drag_stopped_by(egui::PointerButton::Primary)",
                   "self.settle()?", "self.resized_report_panel()"]
        self.assertEqual([resize.index(part) for part in ordered], sorted(resize.index(part) for part in ordered))
        self.assertIn("self.app.test_report.snapshot.is_none()", resize)
        self.assertEqual(source.count("h.resize_report_panel()?"), 1)
        self.assertIn("h.resize_report_panel()?;\n    h.load_report_control()?;", source)
        self.assertEqual(source.count("h.load_report_control()?"), 2)
        self.assertNotIn("set_report_panel_height", source)
        self.assertNotIn("insert_persisted", source)
        self.assertNotIn("PanelState {", source)
        start = source.index("    fn load_report_control(")
        end = source.index("    fn explicit_refresh_reply(", start)
        helper = source[start:end]
        ordered = ["self.resized_report_panel()?", 'self.click_probe("test_report_load", true)?',
                   "self.resized_report_panel()?", "self.read_report_command()?",
                   "self.app.apply_event(event)", "self.source()?", "self.settle()?",
                   "self.resized_report_panel()?", "self.loaded_viewport()"]
        cursor = 0
        for part in ordered:
            cursor = helper.index(part, cursor) + len(part)
        self.assertEqual(helper.count("self.settle()"), 1)
        self.assertNotIn("self.frame(", helper)
        self.assertNotIn("resize_report_panel(", helper)
        settle = source[source.index("    fn settle("):source.index("    fn local(")]
        self.assertIn("for _ in 0..3", settle)
        self.assertEqual(settle.count("self.frame(vec![])?"), 1)
        viewport = source[source.index("    fn loaded_viewport("):source.index("    fn expect_row(")]
        self.assertIn("rect.width() > 0.0", viewport)
        self.assertIn("rect.height() > 0.0", viewport)
        self.assertIn("clip.width() > 0.0", viewport)
        self.assertIn("clip.height() > rect.height()", viewport)
        self.assertIn("clip.contains_rect(rect)", viewport)
        self.assertIn('let name = "test_report_case:0";', viewport)
        self.assertIn("const REPORT_PANEL_HEIGHT: f32 = 480.0;", source)
        self.assertIn("egui::vec2(1000.0, 820.0)", source)


if __name__ == "__main__":
    unittest.main()
