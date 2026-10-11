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

import search_navigation_acceptance as proof


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

    def test_fixed_twelve_request_ledger_and_fourteen_cases(self):
        expected = proof.expected_receipt("linux")
        self.assertEqual(expected["cases"], [
            "explicit_trust_off_search", "exact_path_line_identity_order", "arrow_boundary_clamp",
            "home_end_reveal", "dirty_enter_zero_read_exact_back", "unopened_enter_exact_one_read",
            "ready_batch_capture_rejected", "newer_search_rejects_late_reply",
            "stale_session_capture_rejected", "fresh_reconnect_navigation",
            "stale_query_capture_rejected", "empty_results_inert",
            "dirty_full_selection_native_undo_redo", "exact_ledger_disk_and_owner_cleanup",
        ])
        self.assertEqual(len(set(expected["cases"])), 14)
        self.assertEqual(len(expected["operation_ledger"]), 12)
        self.assertEqual([item["op"] for item in expected["operation_ledger"]],
                         ["Hello", "List", "Read", "Search", "Read", "Search", "Search", "Search",
                          "Hello", "List", "Search", "Search"])
        self.assertEqual([item["phase"] for item in expected["operation_ledger"]],
                         ["setup"] * 4 + ["activation"] + ["refresh"] * 3 + ["reconnect"] * 3 + ["empty"])
        self.assertEqual(len(expected), 57)
        self.assertEqual(expected["source_version"], "0.57.4")
        self.assertEqual((expected["requests"], expected["connections"], expected["agents_reaped"]), (12, 2, 2))
        self.assertEqual((expected["hello"], expected["list"], expected["search"], expected["read"],
                          expected["write"], expected["other_operations"]), (2, 2, 6, 2, 0, 0))
        self.assertEqual((expected["movement_operations"], expected["dirty_activation_reads"],
                          expected["unopened_activation_reads"]), (0, 0, 1))
        self.assertEqual((expected["shared_watchdog_seconds"], expected["total_budget_seconds"],
                          expected["request_admission_seconds"], expected["cleanup_timeout_seconds"]),
                         (60, 65, 30, 5))
        self.assertEqual((expected["result_rows"], expected["fixture_files"]), (192, 2))
        self.assertTrue(all(set(item) == {"source", "sha256"} for item in expected["source_hashes"]))
        self.assertNotIn("needle café", json.dumps(expected, ensure_ascii=False))
        self.assertEqual(expected["source_hashes"], [
            {"source": "draft", "sha256": "78c57b8c218924bacc77f4631c0200873bc9e138e90ca65792580a67c20d0d20"},
            {"source": "unopened", "sha256": "1adb1140e28114b6108eb9e605946a69540b45567798195c84d42d6a7f7ca316"},
        ])

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
            good.replace(proof.PREFIX.encode(), b"Search_navigation_acceptance "),
            good.replace(proof.PREFIX.encode(), b"search_navigation_acceptance\t"),
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
        source = (root / "crates/app/src/search_navigation_process_tests.rs").read_text(encoding="utf-8")
        tests = re.findall(r'#\[test\]\s*#\[ignore = "[^"\n]+"\]\s*fn (\w+)\(', source)
        self.assertEqual(["search_navigation_process_tests::" + name for name in tests],
                         [proof.TEST_NAME])
        self.assertEqual(source.count("#[test]"), 1)
        case_block = source[source.index("const CASES"):source.index("type Check")]
        self.assertEqual(re.findall(r'    "([a-z_]+)",', case_block), list(proof.CASES))
        self.assertIn("CEDAR_SEARCH_NAVIGATION_AGENT_BIN", source)
        self.assertIn('eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);', source)
        self.assertIn('mod search_navigation_process_tests;',
                      (root / "crates/app/src/lib.rs").read_text(encoding="utf-8"))


class ArtifactProvenanceTests(unittest.TestCase):
    def test_rejects_wrong_source_binary_duplicate_artifact_or_non_test(self):
        with tempfile.TemporaryDirectory(prefix="search-artifact-") as directory:
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
        with tempfile.TemporaryDirectory(prefix="search-source-") as directory:
            root = Path(directory).resolve()
            path = root / "Cargo.toml"
            with self.assertRaises(proof.AcceptanceError):
                proof.validate_source_version(root)
            for invalid in (b"", b"\xff", b"[", b'workspace = "wrong"',
                            b'[workspace]\npackage = false',
                            b'[workspace.package]\nversion = 54',
                            b'[workspace.package]\nversion = "0.53.4"',
                            b'[workspace.package]\nversion = "0.57.4-extra"',
                            b'[workspace.package]\nversion = "0.57.4"\nversion = "0.57.4"',
                            b" " * (64 * 1024 + 1)):
                path.write_bytes(invalid)
                with self.subTest(data=invalid[:60]), self.assertRaises(proof.AcceptanceError):
                    proof.validate_source_version(root)
            path.write_text('[workspace.package]\nversion = "0.57.4"\n')
            proof.validate_source_version(root)

    def test_agent_digest_is_exact_nonempty_bounded_and_detects_same_size_change(self):
        with tempfile.TemporaryDirectory(prefix="search-agent-") as directory:
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
        with tempfile.TemporaryDirectory(prefix="search-agent-link-") as directory:
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
                 missing_agent=False, invalid_artifact=False, source_version="0.57.4",
                 changed_agent=False, changed_source=False):
        with tempfile.TemporaryDirectory(prefix="cedar-search-navigation-driver-") as directory:
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
                self.assertEqual(environment["CEDAR_SEARCH_NAVIGATION_AGENT_BIN"], str(agent))
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
                    mock.patch.dict(proof.os.environ, {"CEDAR_SEARCH_NAVIGATION_AGENT_BIN": "wrong",
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
                self.assertIn("14 cases, 12 explicit requests, 0 movement requests, 2 agents reaped", output)

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


class DiagnosticTests(unittest.TestCase):
    def root(self):
        return Path(__file__).resolve().parent.parent

    def progress(self, **changes):
        return dict(schema=1, completed_cases=6, requests=5, connections=1,
                    agents_reaped=0, cleanup_uncertain=False, **changes)

    def test_progress_exact_types_and_bounds(self):
        import search_navigation_diagnostics as diagnostic
        value = self.progress()
        line = diagnostic.PROGRESS_PREFIX + json.dumps(value)
        self.assertEqual(diagnostic.progress_context(line), {"kind": "native_progress", **value})
        for changed in mutations(value):
            # Integer changes within the schema's bounded runtime domain are
            # valid progress; every other structural/type mutation is rejected.
            valid = (type(changed) is dict and set(changed) == diagnostic.PROGRESS_KEYS
                     and all(type(changed[key]) is int and 0 <= changed[key] <= maximum
                             for key, maximum in (("schema", 1), ("completed_cases", 14),
                                                  ("requests", 12), ("connections", 2), ("agents_reaped", 2)))
                     and changed["schema"] == 1 and type(changed["cleanup_uncertain"]) is bool
                     and changed["agents_reaped"] <= changed["connections"])
            result = diagnostic.progress_context(diagnostic.PROGRESS_PREFIX + json.dumps(changed))
            self.assertEqual(result is not None, valid)
        self.assertIsNone(diagnostic.progress_context(line + "x" * 1024))
        self.assertIsNone(diagnostic.progress_context(line.replace('"schema": 1', '"schema": 1, "schema": 1')))
        self.assertIsNone(diagnostic.progress_context(line.replace('"schema": 1', '"schema": NaN')))

    def test_native_failure_order_preserves_context_and_cannot_be_success(self):
        import search_navigation_diagnostics as diagnostic
        output = ("running 1 test\ntest " + proof.TEST_NAME + " ... "
                  + diagnostic.PROGRESS_PREFIX + json.dumps(self.progress())
                  + '\nError: "unopened activation did not dispatch its one exact Read"\n'
                  + "FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; "
                  + "1900 filtered out; finished in 0.02s\n").encode()
        with self.assertRaises(proof.AcceptanceError):
            proof.validate_execution(output, "linux")
        items, _, _ = diagnostic.sanitize_output(output, self.root(), "runtime")
        self.assertEqual([item["kind"] for item in items], ["native_progress", "native_check"])
        self.assertEqual(items[0]["completed_cases"], 6)
        source = (self.root() / diagnostic.SOURCE).read_text(encoding="utf-8")
        self.assertIn("if !self.receipt_emitted {\n            self.progress();", source)
        self.assertEqual(source.count("self.progress();"), 1)
        self.assertIn('println!("search_navigation_acceptance {receipt}");\n    h.receipt_emitted = true;', source)

    def test_static_native_error_and_progress_are_sanitized_without_fixture_text(self):
        import search_navigation_diagnostics as diagnostic
        message = "unopened activation did not dispatch its one exact Read"
        value = self.progress()
        output = (f'Error: "{message}"\n' + diagnostic.PROGRESS_PREFIX + json.dumps(value)
                  + '\nprivate fixture /home/person/token-secret\n'
                  + 'Error: "unreviewed user data"\n').encode()
        items, suppressed, clipped = diagnostic.sanitize_output(output, self.root(), "runtime")
        self.assertEqual(len(items), 2)
        self.assertEqual(items[0]["message"], message)
        self.assertEqual(items[0]["source"], diagnostic.SOURCE)
        self.assertIs(type(items[0]["line"]), int)
        self.assertEqual(items[1], {"kind": "native_progress", **value})
        self.assertEqual(suppressed, 2)
        self.assertFalse(clipped)
        self.assertNotIn("secret", json.dumps(items))
        self.assertNotIn("report_triage", json.dumps(items))

    def test_native_allowlist_is_present_and_does_not_accept_report_only_errors(self):
        import search_navigation_diagnostics as diagnostic
        source = (self.root() / diagnostic.SOURCE).read_text(encoding="utf-8")
        self.assertTrue(diagnostic.NATIVE_ERRORS)
        for message in diagnostic.NATIVE_ERRORS:
            self.assertIn('"' + message + '"', source)
        report = b'Error: "report snapshot absent"\n'
        self.assertEqual(diagnostic.sanitize_output(report, self.root(), "runtime"), ([], 1, False))

    def test_oversize_invalid_utf8_unknown_and_extra_progress_never_escape(self):
        import search_navigation_diagnostics as diagnostic
        value = {**self.progress(), "private": "token-secret"}
        output = (b"x" * 65537 + b"\n\xff\n"
                  + (diagnostic.PROGRESS_PREFIX + json.dumps(value)).encode())
        items, suppressed, clipped = diagnostic.sanitize_output(output, self.root(), "runtime")
        self.assertEqual(items, [])
        self.assertEqual(suppressed, 3)
        self.assertTrue(clipped)

    def test_failure_receipt_keeps_native_ownership_unverified_after_direct_reap(self):
        import search_navigation_diagnostics as diagnostic
        from report_triage_diagnostics import CapturedFailure, StageFailure
        output = (diagnostic.PROGRESS_PREFIX + json.dumps(self.progress()) + "\nprivate-value\n").encode()
        error = StageFailure("runtime", CapturedFailure("subprocess_nonzero", output,
                             returncode=1, eof=True, cleanup="reaped"), self.root(), 90, 256 * 1024)
        receipt = diagnostic.failure_artifact(error)
        self.assertEqual(receipt["acceptance"], "search_navigation_diagnostic")
        self.assertEqual(receipt["native_cleanup"], "unverified")
        self.assertEqual(receipt["direct_child_cleanup"], "reaped")
        self.assertTrue(receipt["capture_complete"])
        self.assertEqual(receipt["captured_bytes"], len(output))
        self.assertNotIn("private-value", json.dumps(receipt))
        self.assertEqual(len(receipt["raw_output_omitted_sha256"]), 64)
        with tempfile.TemporaryDirectory(prefix="search-diagnostic-") as directory:
            path = Path(directory).resolve()
            self.assertEqual(diagnostic.write_failure_artifact(error, str(path)), "written")
            saved = path / diagnostic.ARTIFACT_NAME
            self.assertLessEqual(saved.stat().st_size, diagnostic.MAX_ARTIFACT_BYTES)
            self.assertEqual(json.loads(saved.read_text()), receipt)
            self.assertEqual(diagnostic.write_failure_artifact(error, str(path)), "unavailable")
        self.assertEqual(diagnostic.write_failure_artifact(error, None), "disabled")
        self.assertEqual(diagnostic.write_failure_artifact(error, "relative"), "unavailable")

    def test_compile_diagnostics_retain_only_error_code_and_verified_source_location(self):
        import search_navigation_diagnostics as diagnostic
        value = {"reason": "compiler-message", "message": {
            "level": "error", "code": {"code": "E0308"}, "rendered": "private body",
            "message": "secret-token", "spans": [{"file_name": diagnostic.SOURCE,
                                                     "line_start": 1, "column_start": 1}]}}
        items, _, _ = diagnostic.sanitize_output(json.dumps(value).encode(), self.root(), "compile")
        self.assertEqual(items, [{"kind": "compiler_error", "code": "E0308", "locations": [
            {"source": diagnostic.SOURCE, "line": 1, "column": 1}]}])

    def test_main_writes_only_sanitized_feature_local_failure_artifact(self):
        from report_triage_diagnostics import CapturedFailure, StageFailure
        failure = StageFailure("runtime", CapturedFailure("subprocess_timeout", b"private-output"),
                               self.root(), 90, 256 * 1024)
        error_output = io.StringIO()
        with mock.patch.object(proof.sys, "argv", ["driver"]), \
                mock.patch.object(proof, "run", side_effect=failure), \
                mock.patch.object(proof, "write_failure_artifact", return_value="written") as write, \
                mock.patch.object(proof.sys, "stderr", error_output), \
                mock.patch.dict(proof.os.environ, {"CEDAR_SEARCH_NAVIGATION_DIAGNOSTIC_DIR": "/sanitized"}):
            self.assertEqual(proof.main(), 1)
        write.assert_called_once_with(failure, "/sanitized")
        self.assertNotIn("private-output", error_output.getvalue())
        self.assertIn("sanitized_diagnostic=written", error_output.getvalue())


if __name__ == "__main__":
    unittest.main()
