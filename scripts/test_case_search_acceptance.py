#!/usr/bin/env python3
"""Independent contract and bounded-driver mocks; never build or run an agent."""
from contextlib import redirect_stderr, redirect_stdout
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import case_search_acceptance as proof
from report_triage_diagnostics import CapturedFailure


DEFAULT_RECEIPT = object()


def execution(receipt=DEFAULT_RECEIPT, platform="linux", inline=True):
    value = proof.expected_receipt(platform) if receipt is DEFAULT_RECEIPT else receipt
    selected = "test " + proof.TEST_NAME + " ... "
    encoded = proof.PREFIX + json.dumps(value, ensure_ascii=False)
    body = selected + encoded + "\nok\n" if inline else selected + "ok\n" + encoded + "\n"
    return ("running 1 test\n" + body + "\ntest result: ok. 1 passed; 0 failed; "
            "0 ignored; 0 measured; 999 filtered out; finished in 0.01s\n").encode("utf-8")


def artifacts(root, platform="linux"):
    suffix = ".exe" if platform == "windows" else ""
    events = []
    for name, crate, kind, source in (
        ("stdio_roundtrip", "client", ["test"], "tests/stdio_roundtrip.rs"),
        ("cedar_app", "app", ["lib"], "src/lib.rs"),
    ):
        executable = root / "target/debug/deps" / (name + "-abcdef" + suffix)
        executable.parent.mkdir(parents=True, exist_ok=True)
        executable.write_bytes(b"mock artifact; never executed")
        events.append({"reason": "compiler-artifact",
                       "manifest_path": str(root / "crates" / crate / "Cargo.toml"),
                       "target": {"name": name, "kind": kind,
                                  "src_path": str(root / "crates" / crate / source)},
                       "profile": {"test": True}, "executable": str(executable)})
    return events


def encoded_artifacts(events):
    return ("\n".join(json.dumps(event) for event in events) + "\n").encode("utf-8")


def nested_values(value, path=()):
    yield path, value
    if type(value) is dict:
        for key, child in value.items():
            yield from nested_values(child, path + (key,))
    elif type(value) is list:
        for index, child in enumerate(value):
            yield from nested_values(child, path + (index,))


def replaced(value, path, replacement):
    if not path:
        return replacement
    changed = copy.deepcopy(value)
    target = changed
    for key in path[:-1]:
        target = target[key]
    target[path[-1]] = replacement
    return changed


class FixedContractTests(unittest.TestCase):
    def test_exact_identity_cases_routes_and_twelve_operations(self):
        value = proof.expected_receipt()
        self.assertEqual(proof.QUERY, "K [x].* café 😀")
        self.assertEqual(proof.TEST_NAME,
                         "case_search_process_tests::normal_agent_case_search_is_explicit_local_and_source_bound")
        self.assertEqual(proof.PREFIX, "case_search_acceptance ")
        self.assertEqual(proof.PROGRESS_PREFIX, "case_search_progress ")
        self.assertEqual(value["acceptance"], "case_search")
        self.assertEqual(value["schema"], 1)
        self.assertEqual(value["source_version"], "0.79.0")
        self.assertEqual(value["cases"], [
            "trust_off_normal_agent", "checked_legacy_root", "checked_filtered_route",
            "shared_native_case_controls_local", "unchecked_directory_echo",
            "local_grouping_and_movement", "dirty_preview_and_return", "unopened_preview",
            "toggle_back_retires_search_open", "fresh_root_cached_open_exact_back_undo",
            "nested_reply_retired_before_delivery", "root_basename_mask", "complete_no_match",
            "local_edit_retirement", "retained_shell_and_provider_state",
            "exact_ledger_disk_and_owner_cleanup",
        ])
        self.assertEqual(value["cases_executed"], 16)
        self.assertEqual([(row["phase"], row["op"]) for row in value["operation_ledger"]], [
            ("setup", "Hello"), ("setup", "List"), ("setup", "Read"), ("legacy", "Search"),
            ("checked_directory", "SearchIn"), ("directory", "SearchCase"),
            ("preview", "Read"), ("retired_open", "Read"), ("root", "SearchCase"),
            ("nested", "SearchCase"), ("root_mask", "SearchCase"), ("empty", "SearchCase"),
        ])
        self.assertEqual(value["operation_ledger"][:4], [
            {"phase": "setup", "op": "Hello"},
            {"phase": "setup", "op": "List", "source": "root_directory"},
            {"phase": "setup", "op": "Read", "source": "origin"},
            {"phase": "legacy", "op": "Search", "query": "K [x].* café 😀", "limit": 500},
        ])
        filters = [("src Ω", ["*.rs"]), ("src Ω", ["*.rs"]), ("", []),
                   ("src Ω/nested", ["*.rs", "*.md"]), ("", ["*.rs"]), ("src Ω", ["*.zz"])]
        actual = [row["request"] for row in value["operation_ledger"] if "request" in row]
        expected = []
        for index, (scope, masks) in enumerate(filters):
            request = {"query": "K [x].* café 😀", "scope": scope, "masks": masks, "limit": 500}
            if index:
                request["case_sensitive"] = False
            expected.append(request)
        self.assertEqual(actual, expected)
        self.assertEqual(value["operation_ledger"][6:8], [
            {"phase": "preview", "op": "Read", "source": "target"},
            {"phase": "retired_open", "op": "Read", "source": "target"},
        ])

    def test_fixture_bytes_lengths_and_literal_source_hashes(self):
        expected = [
            ("origin", "src Ω/a origin.rs", "saved", "K", 71,
             "766ed4ad56af0f14d6713b9f8950ad9b252dab1b5b70b05891f730324a0fb548"),
            ("target", "src Ω/b target.rs", "target", "K", 68,
             "4ef314f4e24a6478749a0e5723718853df79011abc5d8ce3504dbf24e8ad3855"),
            ("nested", "src Ω/nested/c note.md", "nested", "k", 66,
             "1107db7a6383b7b362683671b9b35df0a8bdd8dd306bb487b16402e103a5fa50"),
            ("root", "root.rs", "root", "K", 62,
             "53a3c5dbd633670a27a3ac40a8df8965427684e0757eaecd3b502a4af6bd9e9e"),
            ("outside", "outside/d untouched.txt", "outside", "k", 68,
             "22d93725f3dc002091160f5be9c950da1a6d63d4f0b26b74c18dae7b81407f38"),
            ("masked", "src Ω/z masked.RS", "masked", "K", 68,
             "1bcffb10c0a4d7e77ab1da673f06dcd65aacca6e56118ff13debaf5f4de38a1d"),
        ]
        self.assertEqual(proof.FIXTURES, tuple(row[:4] for row in expected))
        self.assertEqual(proof.expected_receipt()["source_hashes"], [
            {"source": source, "sha256": digest} for source, _path, _header, _variant, _length, digest in expected])
        for source, _path, header, variant, length, _digest in expected:
            with self.subTest(source=source):
                data = proof.fixture_bytes(source, header, variant)
                self.assertEqual(len(data), length)
                self.assertFalse(data.startswith(b"\xef\xbb\xbf"))
                self.assertEqual(data.decode("utf-8"),
                                 f"{header} Ω😀\r\n{variant} [x].* café 😀 {source}\r\n"
                                 "K xZZ café 😀 decoy\r\n" + ("last\r\n" if source == "origin" else ""))

    def test_independent_search_rows_original_text_sorting_and_counters(self):
        rows = {
            "origin": {"path": "src Ω/a origin.rs", "line": 2, "text": "K [x].* café 😀 origin"},
            "target": {"path": "src Ω/b target.rs", "line": 2, "text": "K [x].* café 😀 target"},
            "nested": {"path": "src Ω/nested/c note.md", "line": 2, "text": "k [x].* café 😀 nested"},
            "root": {"path": "root.rs", "line": 2, "text": "K [x].* café 😀 root"},
            "outside": {"path": "outside/d untouched.txt", "line": 2, "text": "k [x].* café 😀 outside"},
            "masked": {"path": "src Ω/z masked.RS", "line": 2, "text": "K [x].* café 😀 masked"},
        }
        specification = [
            ("legacy", "Legacy", 2, 2, ["root", "origin"], None, None),
            ("checked_directory", "ScopedV1", 1, 1, ["origin"], 5, 139),
            ("directory", "CaseV1", 2, 2, ["origin", "target"], 5, 139),
            ("root", "CaseV1", 6, 6, ["outside", "root", "origin", "target", "nested", "masked"], 9, 403),
            ("nested", "CaseV1", 1, 0, ["nested"], 1, 66),
            ("root_mask", "CaseV1", 3, 3, ["root", "origin", "target"], 9, 201),
            ("empty", "CaseV1", 0, 0, [], 5, 0),
        ]
        self.assertEqual(proof.expected_receipt()["search_observations"], [
            {"phase": phase, "route": route, "returned_rows": returned, "accepted_rows": accepted,
             "matches": [rows[source] for source in sources], "examined_entries": entries, "read_bytes": count}
            for phase, route, returned, accepted, sources, entries, count in specification])

    def test_independent_counts_budgets_and_preservation_claims(self):
        numbers = {
            "requests": 12, "hello": 1, "list": 1, "read": 3, "search": 1,
            "search_in": 1, "search_case": 5, "write": 0, "other_operations": 0,
            "connection_attempts": 1, "connections": 1, "agents_reaped": 1, "fixture_files": 6,
            "movement_operations": 0, "filter_edit_operations": 0, "toggle_operations": 0,
            "dirty_preview_reads": 0, "unopened_preview_reads": 1,
            "retired_unopened_open_reads": 1, "fresh_cached_open_reads": 0,
            "shell_transition_operations": 0, "shared_active_seconds": 340,
            "request_admission_seconds": 30, "cleanup_timeout_seconds": 30, "proof_watchdog_seconds": 390,
        }
        claims = """trust_off no_language_session no_recovery_actor production_native_frames
            recording_worker_client_bridge default_match_case_checked shared_match_case_controls
            native_pointer_space_enter_toggle exact_scoped_and_case_echo_validated
            literal_metacharacters_preserved original_unicode_snippets_preserved mask_case_sensitive
            complete_no_match toggle_back_does_not_revive retired_open_cached_inactive
            retired_open_no_activation_or_history retired_case_app_job_drained
            retired_case_no_active_rows_preview_or_error dirty_text_and_baseline_preserved
            edit_version_preserved_between_edits full_selection_affinity_preserved
            complete_native_history_preserved native_undo_redo_preserved source_disk_unchanged
            exact_operation_ledger_verified optional_font_probe_untriggered
            shell_project_focus_retains_find shell_find_reopen_query_fallback shell_hide_exact_editor_return
            shell_provider_retains_match_case shell_state_preserved owned_children_reaped watchdog_joined
            fixture_removed source_agent_version_match success""".split()
        for platform in ("linux", "windows"):
            value = proof.expected_receipt(platform)
            self.assertEqual(value["os"], platform)
            for name, required in numbers.items():
                self.assertIs(type(value[name]), int)
                self.assertEqual(value[name], required)
            for name in claims:
                self.assertIs(value[name], True)
        self.assertEqual({**proof.expected_receipt(), "os": "windows"}, proof.expected_receipt("windows"))


class ReceiptTests(unittest.TestCase):
    def test_inline_and_standalone_success_on_both_platforms(self):
        for platform in ("linux", "windows"):
            for inline in (False, True):
                with self.subTest(platform=platform, inline=inline):
                    self.assertEqual(proof.validate_execution(execution(platform=platform, inline=inline), platform),
                                     proof.expected_receipt(platform))

    def test_every_object_missing_extra_or_wrong_type_is_rejected(self):
        receipt = proof.expected_receipt()
        for path, value in nested_values(receipt):
            if type(value) is not dict:
                continue
            variants = [{**value, "unapproved_extra": True}, [], None]
            variants.extend({key: item for key, item in value.items() if key != removed} for removed in value)
            for changed in variants:
                with self.subTest(path=path, keys=list(changed) if type(changed) is dict else changed), \
                        self.assertRaises(proof.AcceptanceError):
                    proof.validate_execution(execution(replaced(receipt, path, changed)))

    def test_every_leaf_value_type_bool_coercion_and_order_is_rejected(self):
        receipt = proof.expected_receipt()
        for path, value in nested_values(receipt):
            if type(value) is dict:
                continue
            if type(value) is list:
                variants = [None, value + [None]]
                if value:
                    variants.append(value[:-1])
                if len(value) > 1 and value != list(reversed(value)):
                    variants.append(list(reversed(value)))
            elif type(value) is bool:
                variants = [not value, int(value), "true" if value else "false", None]
            elif type(value) is int:
                variants = [value + 1, float(value), bool(value), str(value), None]
            elif type(value) is str:
                variants = [value + "wrong", 0, None]
            else:
                variants = [0, False, "null", []]
            for changed in variants:
                with self.subTest(path=path, changed=changed), self.assertRaises(proof.AcceptanceError):
                    proof.validate_execution(execution(replaced(receipt, path, changed)))

    def test_request_echo_masks_order_and_case_mode_are_exact(self):
        receipt = proof.expected_receipt()
        invalid = []
        for index in (4, 5, 8, 9, 10, 11):
            for field, value in (("query", "k [x].* café 😀"), ("scope", "wrong"),
                                 ("masks", ["*"]), ("limit", 499), ("limit", True)):
                invalid.append(replaced(receipt, ("operation_ledger", index, "request", field), value))
            if index != 4:
                for value in (True, 0, 1, None, "false"):
                    invalid.append(replaced(receipt, ("operation_ledger", index, "request", "case_sensitive"), value))
        invalid.append(replaced(receipt, ("operation_ledger", 9, "request", "masks"), ["*.md", "*.rs"]))
        for changed in invalid:
            with self.subTest(ledger=changed["operation_ledger"]), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(execution(changed))

    def test_wrong_test_zero_tests_duplicate_receipt_and_nonreceipt_progress_fail(self):
        good = execution()
        extra = (proof.PREFIX + json.dumps(proof.expected_receipt()) + "\n").encode()
        invalid = [
            good.replace(b"running 1 test", b"running 0 tests"),
            good.replace(b"running 1 test", b"running 2 tests"),
            good.replace(b"1 passed", b"0 passed"),
            good.replace(b"0 ignored", b"1 ignored"),
            good.replace(b"test result: ok.", b"test result: FAILED."),
            good.replace(proof.TEST_NAME.encode(), b"other_test"),
            good.replace(proof.TEST_NAME.encode(), proof.TEST_NAME.encode() + b"_suffix"),
            good.replace(proof.PREFIX.encode(), proof.PROGRESS_PREFIX.encode()),
            good + extra, good + b"running 1 test\n", good + b"test other ... ok\n",
            good.replace(b" ... " + proof.PREFIX.encode(), b" ... diagnostic " + proof.PREFIX.encode()),
            good.replace(b"\ntest result:", b"\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 1s\ntest result:"),
        ]
        for data in invalid:
            with self.subTest(data=data[:120]), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(data)

    def test_duplicate_escaped_duplicate_nonfinite_and_invalid_json_fail(self):
        good = execution()
        invalid = [
            good.replace(b'{', b'{"duplicate": 1, "duplicate": 2,', 1),
            good.replace(b'{', b'{"schema": 1, "\\u0073chema": 1,', 1),
            good.replace(b'"query":', b'"\\u0071uery": "wrong", "query":', 1),
            good.replace(b'{', b'{"bad": NaN,', 1),
            good.replace(b'{', b'{"bad": Infinity,', 1),
            good.replace(b'{', b'{"bad": -Infinity,', 1),
            good.replace(b'"requests": 12', b'"requests": 1e9999', 1),
            good.replace(b'{', b'[', 1),
            good.replace(b'{', b'{invalid,', 1),
            good.replace(b'"schema": 1', b'"schema": null trailing', 1),
        ]
        for data in invalid:
            with self.subTest(data=data[:200]), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(data)

    def test_failure_progress_before_or_after_success_never_becomes_success(self):
        for platform in ("linux", "windows"):
            for receipt_inline in (False, True):
                good = execution(platform=platform, inline=receipt_inline)
                for completed_cases in (0, 5):
                    progress = (proof.PROGRESS_PREFIX + json.dumps({
                        "schema": 1, "completed_cases": completed_cases, "requests": 0,
                        "connection_attempts": 0, "connections": 0, "agents_reaped": 0,
                        "cleanup_uncertain": False, "failure_stage": "setup",
                    })).encode("utf-8")
                    marked_line = next(line for line in good.splitlines() if proof.PREFIX.encode() in line)
                    variants = {
                        "separate_before": progress + b"\n" + good,
                        "separate_after": good + progress + b"\n",
                        "beside_receipt_before": good.replace(marked_line, progress + b" " + marked_line),
                        "beside_receipt_after": good.replace(marked_line, marked_line + b" " + progress),
                        "inline_before": b"diagnostic " + progress + b"\n" + good,
                        "inline_after": good + b"diagnostic " + progress + b"\n",
                    }
                    for position, output in variants.items():
                        with self.subTest(platform=platform, receipt_inline=receipt_inline,
                                          completed_cases=completed_cases, position=position), \
                                self.assertRaisesRegex(proof.AcceptanceError,
                                                       "^failure_progress_in_success_output$"):
                            proof.validate_execution(output, platform)

    def test_utf8_size_and_unsupported_platform_fail_closed(self):
        for value in (execution() + b"\xff", execution().decode(), bytearray(execution()),
                      b"x" * (proof.MAX_RUNTIME_BYTES + 1)):
            with self.subTest(kind=type(value)), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(value)
        with self.assertRaises(proof.AcceptanceError):
            proof.expected_receipt("darwin")
        with self.assertRaises(proof.AcceptanceError):
            proof.validate_execution(execution(platform="windows"), "linux")


class ArtifactTests(unittest.TestCase):
    def test_exact_aggregate_artifacts_with_normal_library_on_both_platforms(self):
        self.assertEqual(proof.compile_command(), ["cargo", "test", "--workspace", "--all-targets",
                                                  "--all-features", "--locked", "--no-run", "--message-format=json"])
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                events = artifacts(root, platform)
                normal = copy.deepcopy(events[1])
                normal["profile"]["test"] = False
                normal["executable"] = None
                actual = proof.select_artifacts(encoded_artifacts([normal] + events), root, platform)
                self.assertEqual(actual, {event["target"]["name"]: Path(event["executable"]) for event in events})

    def test_missing_duplicate_wrong_source_type_and_path_artifacts_fail(self):
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                events = artifacts(root, platform)
                invalid = [[], events[:1], events[1:], events + [events[0]], events + [events[1]]]
                for index in (0, 1):
                    for field, value in (("manifest_path", "/wrong/Cargo.toml"), ("executable", None),
                                         ("executable", "/wrong/test-abcdef"), ("executable", "relative-abcdef"),
                                         ("executable", str(root / "target/debug/deps" / "cedar_app-nothex")),
                                         ("profile", {"test": 1}), ("profile", {"test": False}),
                                         ("profile", None), ("target", [])):
                        invalid.append(replaced(events, (index, field), value))
                    for field, value in (("kind", ["bin"]), ("src_path", "/wrong/source.rs"),
                                         ("name", "other_test"), ("name", None), ("name", [])):
                        invalid.append(replaced(events, (index, "target", field), value))
                for changed in invalid:
                    with self.subTest(events=changed), self.assertRaises(proof.AcceptanceError):
                        proof.select_artifacts(encoded_artifacts(changed), root, platform)

    def test_artifact_missing_file_directory_and_symlink_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            events = artifacts(root)
            executable = Path(events[0]["executable"])
            executable.unlink()
            with self.assertRaises(proof.AcceptanceError):
                proof.select_artifacts(encoded_artifacts(events), root, "linux")
            executable.mkdir()
            with self.assertRaises(proof.AcceptanceError):
                proof.select_artifacts(encoded_artifacts(events), root, "linux")
            executable.rmdir()
            try:
                executable.symlink_to(Path(events[1]["executable"]))
            except OSError:
                return  # Windows runners need not grant symlink creation.
            with self.assertRaises(proof.AcceptanceError):
                proof.select_artifacts(encoded_artifacts(events), root, "linux")

    def test_compile_json_encoding_duplicates_and_size_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            good = encoded_artifacts(artifacts(root))
            invalid = [good + b"\xff", good.replace(b'{', b'{"reason": "x",', 1),
                       good.replace(b'{', b'{"bad": NaN,', 1), b"{invalid}\n",
                       b"x" * (proof.MAX_COMPILE_BYTES + 1)]
            for data in invalid:
                with self.subTest(data=data[:100]), self.assertRaises(proof.AcceptanceError):
                    proof.select_artifacts(data, root, "linux")


class DriverTests(unittest.TestCase):
    def fixture(self, platform="linux", failure=None, category="subprocess_nonzero",
                invalid=None, mutate_agent=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            events = artifacts(root, platform)
            agent = root / "target/release" / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
            agent.parent.mkdir(parents=True)
            agent.write_bytes(b"owned mock release agent; never executed")
            calls = []

            def runner(command, cwd, environment, timeout, limit):
                stage = "compile" if not calls else "runtime"
                calls.append((stage, command, timeout, limit, environment))
                self.assertEqual(cwd, root)
                self.assertEqual(environment["CEDAR_CASE_SEARCH_AGENT_BIN"], str(agent))
                self.assertEqual(environment["CARGO_TARGET_DIR"], str(root / "target"))
                if stage == failure:
                    raise CapturedFailure(category, (proof.PROGRESS_PREFIX + "bounded Ω😀 diagnostic\n").encode(),
                                          returncode=101 if category == "subprocess_nonzero" else None,
                                          eof=category == "subprocess_nonzero",
                                          cleanup="not_started" if category == "subprocess_launch_error" else "reaped",
                                          truncated=category == "subprocess_output_limit")
                if stage == invalid:
                    return b"invalid output\n"
                if stage == mutate_agent:
                    agent.write_bytes(b"changed release bytes")
                return encoded_artifacts(events) if stage == "compile" else execution(platform=platform)

            native_platform = "win32" if platform == "windows" else "linux"
            output = io.StringIO()
            with mock.patch.object(proof.sys, "platform", native_platform), redirect_stdout(output), redirect_stderr(output):
                if failure or invalid or mutate_agent:
                    with self.assertRaises(proof.AcceptanceError) as raised:
                        proof.run(root, runner)
                    error = str(raised.exception)
                else:
                    self.assertEqual(proof.run(root, runner), proof.expected_receipt(platform))
                    error = ""
            self.assertEqual(calls[0][1], ["cargo", "test", "--workspace", "--all-targets", "--all-features",
                                           "--locked", "--no-run", "--message-format=json"])
            self.assertEqual(calls[0][2:4], (120, 4 * 1024 * 1024))
            if len(calls) == 2:
                self.assertEqual(calls[1][1], [events[1]["executable"], proof.TEST_NAME, "--ignored", "--exact",
                                               "--test-threads=1", "--nocapture"])
                self.assertEqual(calls[1][2:4], (420, 256 * 1024))
            return calls, output.getvalue(), error

    def test_success_preserves_commands_environment_artifacts_and_bounds(self):
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                calls, output, _ = self.fixture(platform)
                self.assertEqual(len(calls), 2)
                self.assertIn("Match Case acceptance passed", output)

    def test_spawn_nonzero_timeout_and_output_limit_stop_each_stage_without_retry(self):
        for platform in ("linux", "windows"):
            for stage in ("compile", "runtime"):
                for category in ("subprocess_launch_error", "subprocess_nonzero", "subprocess_timeout",
                                 "subprocess_output_limit", "child_cleanup_unverified"):
                    with self.subTest(platform=platform, stage=stage, category=category):
                        calls, output, error = self.fixture(platform, failure=stage, category=category)
                        self.assertEqual(len(calls), 1 if stage == "compile" else 2)
                        self.assertIn("bounded Ω😀 diagnostic", output)
                        self.assertIn(stage + ":" + category, error)
                        self.assertIn("native_cleanup=unverified", error)
                        self.assertNotIn("acceptance passed", output)

    def test_invalid_stage_output_stops_without_retry_or_native_cleanup_claim(self):
        for stage in ("compile", "runtime"):
            with self.subTest(stage=stage):
                calls, output, error = self.fixture(invalid=stage)
                self.assertEqual(len(calls), 1 if stage == "compile" else 2)
                self.assertIn("native_cleanup=unverified", error)
                self.assertNotIn("acceptance passed", output)

    def test_changed_release_agent_is_rejected(self):
        for stage in ("compile", "runtime"):
            with self.subTest(stage=stage):
                calls, output, error = self.fixture(mutate_agent=stage)
                self.assertEqual(len(calls), 1 if stage == "compile" else 2)
                self.assertEqual(error, "release_agent_changed")
                self.assertNotIn("acceptance passed", output)

    def test_missing_empty_and_symlink_release_agent_prevent_any_spawn(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            runner = mock.Mock()
            agent = root / "target/release/cedar-agent"
            agent.parent.mkdir(parents=True)
            with mock.patch.object(proof.sys, "platform", "linux"):
                for data in (None, b""):
                    if data is not None:
                        agent.write_bytes(data)
                    with self.assertRaises(proof.AcceptanceError):
                        proof.run(root, runner)
                agent.unlink()
                real = root / "real-agent"
                real.write_bytes(b"never executed")
                try:
                    agent.symlink_to(real)
                except OSError:
                    pass
                else:
                    with self.assertRaises(proof.AcceptanceError):
                        proof.run(root, runner)
            runner.assert_not_called()

    def test_unsupported_host_prevents_any_spawn(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(proof.sys, "platform", "darwin"):
            runner = mock.Mock()
            with self.assertRaises(proof.AcceptanceError):
                proof.run(directory, runner)
            runner.assert_not_called()


class OutputEncodingTests(unittest.TestCase):
    def stage(self, output, failure=False, completed=None):
        def runner(*_args):
            if failure:
                raise CapturedFailure("subprocess_nonzero", output,
                                      returncode=101, eof=True, cleanup="reaped")
            return output

        def validator(data):
            receipt = proof.validate_execution(data, "windows")
            if completed is not None:
                completed.append(receipt)
            return receipt

        return proof.checked_stage("runtime", [], Path.cwd(), {}, proof.RUNTIME_TIMEOUT,
                                   proof.MAX_RUNTIME_BYTES, validator, runner)

    def cli(self, output, failure=False):
        completed = []
        with io.BytesIO() as out, io.BytesIO() as err:
            with io.TextIOWrapper(out, encoding="cp1252") as stdout, \
                    io.TextIOWrapper(err, encoding="cp1252") as stderr:
                with redirect_stdout(stdout), redirect_stderr(stderr), \
                        mock.patch.object(proof.sys, "argv", ["case_search_acceptance.py"]), \
                        mock.patch.object(proof, "run", side_effect=lambda _root: self.stage(
                            output, failure=failure, completed=completed)):
                    result = proof.main()
                stdout.flush()
                stderr.flush()
                self.assertEqual(stdout.encoding, "utf-8")
                self.assertEqual(stderr.encoding, "utf-8")
                return result, out.getvalue(), err.getvalue(), completed

    def test_unconfigured_cp1252_stdout_fails_after_strict_receipt_validation(self):
        completed = []
        with io.BytesIO() as raw, io.TextIOWrapper(raw, encoding="cp1252") as stream:
            with redirect_stdout(stream), self.assertRaises(UnicodeEncodeError):
                self.stage(execution(platform="windows"), completed=completed)
        self.assertEqual(completed, [proof.expected_receipt("windows")])

    def test_cli_emits_exact_utf8_non_bmp_receipt_and_windows_crlf(self):
        for newline in (b"\n", b"\r\n"):
            with self.subTest(newline=newline):
                output = execution(platform="windows").replace(b"\n", newline)
                result, stdout, stderr, completed = self.cli(output)
                self.assertEqual(result, 0)
                self.assertEqual(stdout, output)
                self.assertEqual(stderr, b"")
                self.assertEqual(completed, [proof.expected_receipt("windows")])
                self.assertEqual(proof.validate_execution(stdout, "windows"), proof.expected_receipt("windows"))

    def test_cli_preserves_invalid_unicode_receipt_and_failure_on_stderr(self):
        receipt = {**proof.expected_receipt("windows"), "success": False}
        output = execution(receipt, platform="windows")
        result, stdout, stderr, completed = self.cli(output)
        self.assertEqual(result, 1)
        self.assertEqual(stdout, b"")
        self.assertEqual(stderr, output + ("Match Case acceptance failed: runtime:fixed_receipt_contract_mismatch; "
                                           "native_cleanup=unverified\n").encode("utf-8"))
        self.assertEqual(completed, [])

    def test_cli_preserves_failure_progress_and_unicode_on_stderr(self):
        for newline in (b"\n", b"\r\n"):
            output = (proof.PROGRESS_PREFIX + '"bounded Ω😀 diagnostic"\n').encode().replace(b"\n", newline)
            result, stdout, stderr, completed = self.cli(output, failure=True)
            self.assertEqual(result, 1)
            self.assertEqual(stdout, b"")
            self.assertEqual(stderr, output + ("Match Case acceptance failed: runtime:subprocess_nonzero; returncode=101; "
                                               "direct_child_cleanup=reaped; native_cleanup=unverified\n").encode("utf-8"))
            self.assertEqual(completed, [])

    def test_cli_rejects_zero_case_failure_progress_even_with_exact_success_receipt(self):
        output = execution(platform="windows") + (
            proof.PROGRESS_PREFIX + '{"schema":1,"completed_cases":0}\n').encode("utf-8")
        result, stdout, stderr, completed = self.cli(output)
        self.assertEqual(result, 1)
        self.assertEqual(stdout, b"")
        self.assertEqual(stderr, output + ("Match Case acceptance failed: runtime:failure_progress_in_success_output; "
                                           "native_cleanup=unverified\n").encode("utf-8"))
        self.assertEqual(completed, [])


if __name__ == "__main__":
    unittest.main()
