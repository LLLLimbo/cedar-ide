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

import duplicate_lines_acceptance as proof


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

    def test_fixed_six_request_ledger_and_twenty_cases(self):
        expected = proof.expected_receipt("linux")
        self.assertEqual(expected["cases"], [
            'unicode_crlf_collapsed',
            'unicode_lf_collapsed',
            'unicode_crlf_forward_block',
            'unicode_crlf_reversed_block',
            'unicode_lf_forward_block',
            'unicode_lf_reversed_block',
            'upper_endpoint_line_start_excluded',
            'mixed_line_endings_exact',
            'unterminated_nearest_separator',
            'empty_and_trailing_blank',
            'strict_modifiers_and_repeat_refused',
            'mixed_edit_frames_native_only',
            'focus_and_modal_guards',
            'stale_identity_and_selection_guards',
            'byte_and_line_bounds_refused',
            'dirty_baselines_and_other_tab_preserved',
            'atomic_full_selection_undo_redo',
            'stale_reconnect_capture_rejected',
            'history_after_reconnect',
            'duplication_after_reconnect',
        ])
        self.assertEqual(expected["operation_ledger"], [
            {"session": 1, "op": "Hello"}, {"session": 1, "op": "List", "path": ""},
            {"session": 1, "op": "Read", "path": "duplicate 😀/CRLF café.txt"},
            {"session": 1, "op": "Read", "path": "duplicate 😀/LF Ω.txt"},
            {"session": 2, "op": "Hello"}, {"session": 2, "op": "List", "path": ""},
        ])
        self.assertEqual(len(expected), 47)
        self.assertEqual(expected["source_version"], "0.59.3")
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
            good.replace(proof.PREFIX.encode(), b"Duplicate_lines_acceptance "),
            good.replace(proof.PREFIX.encode(), b"duplicate_lines_acceptance\t"),
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
        source = (root / "crates/app/src/duplicate_lines_process_tests.rs").read_text(encoding="utf-8")
        tests = re.findall(r'#\[test\]\s*#\[ignore = "[^"\n]+"\]\s*fn (\w+)\(', source)
        self.assertEqual(["duplicate_lines_process_tests::" + name for name in tests],
                         [proof.TEST_NAME])
        self.assertEqual(source.count("#[test]"), 1)
        case_block = source[source.index("const CASES"):source.index("type Check")]
        self.assertEqual(re.findall(r'    "([a-z_]+)",', case_block), list(proof.CASES))
        self.assertIn("CEDAR_DUPLICATE_LINES_AGENT_BIN", source)
        self.assertIn('eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);', source)
        self.assertIn('mod duplicate_lines_process_tests;',
                      (root / "crates/app/src/lib.rs").read_text(encoding="utf-8"))


class ArtifactProvenanceTests(unittest.TestCase):
    def test_rejects_wrong_source_binary_duplicate_artifact_or_non_test(self):
        with tempfile.TemporaryDirectory(prefix="duplicate-artifact-") as directory:
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
        with tempfile.TemporaryDirectory(prefix="duplicate-source-") as directory:
            root = Path(directory).resolve()
            path = root / "Cargo.toml"
            with self.assertRaises(proof.AcceptanceError):
                proof.validate_source_version(root)
            for invalid in (b"", b"\xff", b"[", b'workspace = "wrong"',
                            b'[workspace]\npackage = false',
                            b'[workspace.package]\nversion = 54',
                            b'[workspace.package]\nversion = "0.53.4"',
                            b'[workspace.package]\nversion = "0.59.3-extra"',
                            b'[workspace.package]\nversion = "0.59.3"\nversion = "0.59.3"',
                            b" " * (64 * 1024 + 1)):
                path.write_bytes(invalid)
                with self.subTest(data=invalid[:60]), self.assertRaises(proof.AcceptanceError):
                    proof.validate_source_version(root)
            path.write_text('[workspace.package]\nversion = "0.59.3"\n')
            proof.validate_source_version(root)

    def test_agent_digest_is_exact_nonempty_bounded_and_detects_same_size_change(self):
        with tempfile.TemporaryDirectory(prefix="duplicate-agent-") as directory:
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
        with tempfile.TemporaryDirectory(prefix="duplicate-agent-link-") as directory:
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
                 missing_agent=False, invalid_artifact=False, source_version="0.59.3",
                 changed_agent=False, changed_source=False):
        with tempfile.TemporaryDirectory(prefix="cedar-duplicate-lines-driver-") as directory:
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
                self.assertEqual(environment["CEDAR_DUPLICATE_LINES_AGENT_BIN"], str(agent))
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
                    mock.patch.dict(proof.os.environ, {"CEDAR_DUPLICATE_LINES_AGENT_BIN": "wrong",
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
                self.assertIn("20 cases, 6 setup requests, 2 agents reaped", output)

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


if __name__ == "__main__":
    unittest.main()
