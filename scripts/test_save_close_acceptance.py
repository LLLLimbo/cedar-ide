#!/usr/bin/env python3
"""Pure adversarial contract/supervisor tests; never compile or run Rust."""
from contextlib import redirect_stderr, redirect_stdout
import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import copy_draft_acceptance as shared
import save_close_acceptance as proof


def payload(receipts=None, platform="linux"):
    if receipts is None:
        receipts = proof.expected_receipts(platform)
    return ("running 1 test\n"
            "test " + proof.TEST_NAME + " ... "
            + "\n".join(proof.PREFIX + json.dumps(receipt, ensure_ascii=False) for receipt in receipts)
            + "\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "1899 filtered out; finished in 0.02s\n").encode("utf-8")


def listing():
    return (proof.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n").encode()


def artifact(root, executable):
    return {"reason": "compiler-artifact", "manifest_path": str(root / "crates/app/Cargo.toml"),
            "target": {"name": "cedar_app", "kind": ["lib"],
                       "src_path": str(root / "crates/app/src/lib.rs")},
            "profile": {"test": True}, "executable": str(executable)}


class ReceiptTests(unittest.TestCase):
    def reject(self, receipts):
        with self.assertRaises(proof.AcceptanceError):
            proof.validate_execution(payload(receipts), "linux")

    def test_exact_both_platform_receipts_with_lf_and_crlf(self):
        for platform in ("linux", "windows"):
            for ending in (b"\n", b"\r\n"):
                with self.subTest(platform=platform, ending=ending):
                    data = payload(platform=platform).replace(b"\n", ending)
                    self.assertEqual(proof.validate_execution(data, platform),
                                     proof.expected_receipts(platform))

    def test_fixture_counts_and_write_contract_are_independently_fixed(self):
        receipts = proof.expected_receipts("linux")
        self.assertEqual([r["test"] for r in receipts], [
            "normal_existing_valid_close", "normal_new_file_valid_close",
            "normal_conflict_retained", "controlled_wrong_digest_unknown_retained",
            "normal_cancel_after_dispatch_retained", "normal_newer_edits_before_ack_retained"])
        self.assertEqual([r["requests"] for r in receipts], [5, 4, 4, 4, 5, 5])
        self.assertEqual([r["list"] for r in receipts], [2, 2, 1, 1, 2, 2])
        self.assertEqual([r["read"] for r in receipts], [1, 0, 1, 1, 1, 1])
        for key, count in {"requests": 27, "hello": 6, "list": 10, "read": 5, "write": 6,
                           "connections": 6, "reaped": 6, "successful_ack_refresh_lists": 4,
                           "recovery_workers_started": 6, "recovery_workers_stopped": 6}.items():
            self.assertEqual(sum(r[key] for r in receipts), count)
        base_digest = hashlib.sha256("original α\nsecond café\nthird line\n".encode()).hexdigest()
        draft_digest = hashlib.sha256("unsaved draft β\nsecond café\nthird line\n".encode()).hexdigest()
        for index, receipt in enumerate(receipts):
            source = "draft.txt" if index == 3 else "source Ω.txt"
            expected = [{"session": 1, "op": "Hello"}, {"session": 1, "op": "List", "path": ""}]
            if index != 1:
                expected.append({"session": 1, "op": "Read", "path": source})
            expected.append({"session": 1, "op": "Write", "path": source,
                             "expected_revision": None if index == 1 else base_digest,
                             "text_sha256": draft_digest})
            if index in (0, 1, 4, 5):
                expected.append({"session": 1, "op": "List", "path": ""})
            self.assertEqual(receipt["operation_ledger"], expected)
            self.assertEqual(receipt["controlled_operation_ledger"],
                             ["Hello", "List", "Read", "Write"] if index == 3 else [])
            self.assertEqual(receipt["write"], 1)
            self.assertEqual(receipt["run"], 0)
            self.assertEqual(receipt["language"], 0)
            self.assertIs(receipt["no_replay"], True)

    def test_successful_close_and_retention_claims_are_branch_specific(self):
        receipts = proof.expected_receipts("linux")
        self.assertEqual([r["target_closed"] for r in receipts], [True, True, False, False, False, False])
        self.assertEqual([r["full_selection_preserved"] for r in receipts],
                         [None, None, True, True, True, True])
        self.assertEqual([r["undo_redo_preserved"] for r in receipts],
                         [None, None, True, True, True, True])
        self.assertEqual([r["source_recovery_state"] for r in receipts], [
            "removed_after_valid_ack", "removed_after_valid_ack", "retained_original_base",
            "retained_original_base", "removed_after_valid_ack", "retained_newer_with_submitted_base"])
        for index in (2, 3, 4, 5):
            bad = copy.deepcopy(receipts)
            bad[index]["target_closed"] = True
            with self.subTest(forged_close=index):
                self.reject(bad)
        for index in (0, 1):
            for field in ("full_selection_preserved", "undo_redo_preserved"):
                bad = copy.deepcopy(receipts)
                bad[index][field] = True
                with self.subTest(nonexistent_target=index, field=field):
                    self.reject(bad)

    def test_cancelled_and_newer_edits_adopt_only_the_submitted_baseline(self):
        receipts = proof.expected_receipts("linux")
        draft = "unsaved draft β\nsecond café\nthird line\n"
        newer = "newer source Ω\nsecond café\nthird line\n"
        draft_digest = hashlib.sha256(draft.encode()).hexdigest()
        newer_digest = hashlib.sha256(newer.encode()).hexdigest()
        for index, text in ((4, draft), (5, newer)):
            receipt = receipts[index]
            self.assertEqual(receipt["target_text_after"], text)
            self.assertEqual(receipt["target_saved_text_after"], draft)
            self.assertEqual(receipt["target_revision_after"], draft_digest)
            self.assertEqual(receipt["source_final_text"], draft)
            self.assertEqual(receipt["source_final_sha256"], draft_digest)
            self.assertEqual(receipt["ack_class"], "valid")
            self.assertIs(receipt["baseline_adopted"], True)
            for key, forged in {
                "target_closed": True, "target_retained": False,
                "target_text_after": None, "target_saved_text_after": None,
                "target_revision_after": None, "baseline_adopted": False,
                "close_before_ack": True, "source_final_text": newer,
                "source_final_sha256": newer_digest, "successful_ack_refresh_lists": 0,
            }.items():
                bad = copy.deepcopy(receipts)
                bad[index][key] = forged
                with self.subTest(case=index, forged=key):
                    self.reject(bad)
        self.assertIs(receipts[4]["cancelled_after_dispatch"], True)
        self.assertIs(receipts[4]["newer_edits_before_ack"], False)
        self.assertIsNone(receipts[4]["source_recovery_text_sha256"])
        self.assertIsNone(receipts[4]["source_recovery_base_sha256"])
        self.assertIsNone(receipts[4]["source_recovery_revision"])
        self.assertIs(receipts[5]["cancelled_after_dispatch"], False)
        self.assertIs(receipts[5]["newer_edits_before_ack"], True)
        self.assertEqual(receipts[5]["source_recovery_text_sha256"], newer_digest)
        self.assertEqual(receipts[5]["source_recovery_base_sha256"], draft_digest)
        self.assertEqual(receipts[5]["source_recovery_revision"], draft_digest)

    def test_unrelated_active_document_is_never_targeted_or_closed(self):
        receipts = proof.expected_receipts("linux")
        for index, receipt in enumerate(receipts):
            self.assertEqual(receipt["target_document"], 1)
            self.assertEqual(receipt["other_document"], 2)
            self.assertEqual(receipt["active_document_before"], 2)
            self.assertEqual(receipt["active_document_after"], 2)
            self.assertEqual(receipt["other"], "other café.txt")
            for field in ("other_state_preserved", "other_recovery_bytes_preserved", "other_disk_absent"):
                self.assertIs(receipt[field], True)
            for field, value in (("target_document", 2), ("other_document", 1),
                                 ("active_document_after", None), ("active_document_after", 1)):
                bad = copy.deepcopy(receipts)
                bad[index][field] = value
                self.reject(bad)
            bad = copy.deepcopy(receipts)
            writing = next(entry for entry in bad[index]["operation_ledger"] if entry["op"] == "Write")
            writing["path"] = "other café.txt"
            self.reject(bad)

    def test_every_field_is_required_and_no_extra_fields_allowed(self):
        for index, receipt in enumerate(proof.expected_receipts("linux")):
            for key in receipt:
                receipts = proof.expected_receipts("linux")
                del receipts[index][key]
                with self.subTest(case=index, missing=key):
                    self.reject(receipts)
            receipts = proof.expected_receipts("linux")
            receipts[index]["extra"] = True
            self.reject(receipts)

    def test_every_scalar_has_exact_type_and_value(self):
        for index, receipt in enumerate(proof.expected_receipts("linux")):
            for key, value in receipt.items():
                if type(value) is list:
                    continue
                bad_values = ([], {}, None, "", 0, 1, 0.0, 1.0, True, False)
                if type(value) is str:
                    bad_values += (value + "!", value.upper())
                elif type(value) is int:
                    bad_values += (value + 1, float(value), str(value))
                for bad in bad_values:
                    if proof.typed_equal(bad, value):
                        continue
                    receipts = proof.expected_receipts("linux")
                    receipts[index][key] = bad
                    with self.subTest(case=index, key=key, bad=bad):
                        self.reject(receipts)

    def test_absent_duplicate_extra_wrong_order_or_wrong_case_receipts_rejected(self):
        good = proof.expected_receipts("linux")
        for bad in ([], good[:-1], good[1:], good + [good[-1]], [good[0]] * 6,
                    list(reversed(good)), [good[1], good[0], *good[2:]]):
            with self.subTest(length=len(bad)):
                self.reject(bad)
        for index in range(6):
            bad = copy.deepcopy(good)
            bad[index]["test"] = "normal_Existing_valid_close"
            self.reject(bad)

    def test_nested_ledgers_require_all_and_only_exact_operations(self):
        good = proof.expected_receipts("linux")
        for index, receipt in enumerate(good):
            for field in ("operation_ledger", "session_ledger"):
                for entry_index, entry in enumerate(receipt[field]):
                    replacements = [None, [], "Hello", {}, {**entry, "extra": False}]
                    for key, value in entry.items():
                        removed = dict(entry)
                        del removed[key]
                        replacements.append(removed)
                        for bad in (None, True, False, 0, 1, "", "../other", "0" * 64,
                                    [], {}, "false", value.upper() if type(value) is str else "absent"):
                            if not proof.typed_equal(bad, value):
                                replacements.append({**entry, key: bad})
                    for replacement in replacements:
                        bad = copy.deepcopy(good)
                        bad[index][field][entry_index] = replacement
                        with self.subTest(case=index, field=field, entry=entry_index, replacement=replacement):
                            self.reject(bad)
            for field in ("operation_ledger", "controlled_operation_ledger", "session_ledger"):
                ledger = receipt[field]
                for replacement in (None, {}, "", False, ledger + [{"op": "Run"}],
                                    ledger[:-1] if ledger else ["Hello"],
                                    list(reversed(ledger)) if len(ledger) > 1 else ["Read"]):
                    bad = copy.deepcopy(good)
                    bad[index][field] = replacement
                    self.reject(bad)

    def test_duplicate_json_keys_nonfinite_and_malformed_json_rejected(self):
        good = payload()
        for bad in (
            good.replace(b'"schema": 1', b'"schema": 1, "schema": 1', 1),
            good.replace(b'"op": "Hello"', b'"op": "Hello", "op": "Hello"', 1),
            good.replace(b'"schema": 1', b'"schema": NaN', 1),
            good.replace(b'"schema": 1', b'"schema": Infinity', 1),
            good.replace(b'"schema": 1', b'"schema": -Infinity', 1),
            good.replace(b'"schema": 1', b'"schema": ', 1),
        ):
            with self.subTest(prefix=bad[:100]), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(bad, "linux")

    def test_raw_json_unmarked_malformed_or_wrong_prefix_not_receipts(self):
        good = payload()
        for bad in (b"", b"\xff", good.decode(), b"x" * (proof.MAX_RUNTIME_BYTES + 1),
                    good.replace(proof.PREFIX.encode(), b""),
                    good.replace(proof.PREFIX.encode(), b"Save_close_acceptance ", 1),
                    good.replace(proof.PREFIX.encode(), b"unverified " + proof.PREFIX.encode(), 1),
                    good.replace(proof.PREFIX.encode(), b"save_close_acceptance\t", 1),
                    good.replace(proof.PREFIX.encode(), proof.PREFIX.encode() + b"[] ", 1),
                    good + good):
            with self.subTest(kind=type(bad)), self.assertRaises(proof.AcceptanceError):
                proof.validate_execution(bad, "linux")

    def test_receipts_need_exactly_one_successful_executed_test(self):
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

    def test_booleans_are_not_integers_even_recursively(self):
        for actual, expected in ((True, 1), (False, 0), ([True], [1]),
                                 ({"nested": [0]}, {"nested": [False]}),
                                 (1.0, 1), ({"a": 1, "b": 2}, {"a": 1})):
            self.assertFalse(proof.typed_equal(actual, expected))


class SelectionTests(unittest.TestCase):
    def test_reviewed_supervision_and_artifact_selection_are_reused_unchanged(self):
        for name in ("bounded_process", "compiled_test", "decode_output", "strict_json",
                     "typed_equal", "test_compile_command", "AcceptanceError"):
            self.assertIs(getattr(proof, name), getattr(shared, name))
        self.assertEqual((proof.COMPILE_TIMEOUT, proof.SELECTION_TIMEOUT,
                          proof.RUNTIME_TIMEOUT, proof.REAP_TIMEOUT), (120, 15, 180, 5))
        self.assertEqual(proof.COMPILE_TIMEOUT + proof.SELECTION_TIMEOUT
                         + proof.RUNTIME_TIMEOUT + 3 * proof.REAP_TIMEOUT, 330)

    def test_compile_and_exact_ignored_listing_are_fixed(self):
        self.assertEqual(proof.test_compile_command(), ["cargo", "test", "-p", "cedar-app", "--lib",
                         "--locked", "--no-run", "--message-format=json"])
        proof.validate_test_listing(listing())
        proof.validate_test_listing(listing().replace(b"\n", b"\r\n"))

    def test_absent_extra_duplicate_wrong_or_malformed_listing_rejected(self):
        for data in (b"", b"\xff", b"x" * (proof.MAX_LIST_BYTES + 1),
                     b"0 tests, 0 benchmarks\n", listing() * 2,
                     listing().replace(proof.TEST_NAME.encode(), b"wrong_test"),
                     listing().replace(proof.TEST_NAME.encode(), shared.TEST_NAME.encode()),
                     listing().replace(b": test", b": benchmark"),
                     listing().replace(b"1 test", b"2 tests"),
                     listing() + b"additional_test: test\n", listing() + b"unexpected output\n"):
            with self.subTest(data=data[:60]), self.assertRaises(proof.AcceptanceError):
                proof.validate_test_listing(data)

    def test_exact_cargo_library_artifact_required_on_both_platforms(self):
        with tempfile.TemporaryDirectory(prefix="cedar-save-close-artifact-") as directory:
            root = Path(directory).resolve()
            deps = root / "target/debug/deps"
            deps.mkdir(parents=True)
            for platform, suffix in (("linux", ""), ("windows", ".exe")):
                executable = deps / ("cedar_app-123abc" + suffix)
                executable.write_bytes(b"synthetic: never executed")
                event = artifact(root, executable)
                self.assertEqual(proof.compiled_test(json.dumps(event).encode(), root, platform), executable)
                dependency = {"reason": "compiler-artifact", "target": {"name": "cedar_client"}}
                data = b"Cargo progress\n" + json.dumps(dependency).encode() + b"\n" + json.dumps(event).encode()
                self.assertEqual(proof.compiled_test(data, root, platform), executable)
                invalid_events = [[], [event, event], [{**event, "executable": None}],
                                  [{**event, "profile": {"test": 1}}],
                                  [{**event, "profile": {"test": False}}],
                                  [{**event, "profile": []}], [{**event, "target": []}],
                                  [{**event, "manifest_path": str(root / "other/Cargo.toml")}],
                                  [{**event, "target": {**event["target"], "src_path": "elsewhere.rs"}}],
                                  [{**event, "target": {**event["target"], "kind": ["bin"]}}],
                                  [{**event, "target": {**event["target"], "name": "other"}}]]
                for events in invalid_events:
                    data = b"\n".join(json.dumps(value).encode() for value in events)
                    with self.subTest(platform=platform, events=events), self.assertRaises(proof.AcceptanceError):
                        proof.compiled_test(data, root, platform)
                for path in (root / executable.name, deps / ("other-123abc" + suffix),
                             deps / "cedar_app-123abc.dll", deps / ("cedar_app-nothex" + suffix),
                             Path("target/debug/deps") / executable.name):
                    with self.subTest(path=path), self.assertRaises(proof.AcceptanceError):
                        proof.compiled_test(json.dumps(artifact(root, path)).encode(), root, platform)
                executable.unlink()
                with self.assertRaises(proof.AcceptanceError):
                    proof.compiled_test(json.dumps(event).encode(), root, platform)

    def test_compile_output_encoding_limit_and_duplicate_fields(self):
        with tempfile.TemporaryDirectory(prefix="cedar-save-close-artifact-") as directory:
            root = Path(directory).resolve()
            for data in (b"\xff", b"x" * (proof.MAX_COMPILE_BYTES + 1),
                         b'{"reason":"compiler-artifact","reason":"compiler-artifact"}',
                         b'{"reason":NaN}', b"{bad json}", b"{}"):
                with self.subTest(data=data[:50]), self.assertRaises(proof.AcceptanceError):
                    proof.compiled_test(data, root, "linux")


class DriverTests(unittest.TestCase):
    def exercise(self, platform="linux", *, stage_failure=None, selection=None, runtime=None,
                 missing_agent=None, invalid_artifact=False):
        with tempfile.TemporaryDirectory(prefix="cedar-save-close-driver-") as directory:
            root = Path(directory).resolve()
            deps = root / "target/debug/deps"
            release = root / "target/release"
            deps.mkdir(parents=True)
            release.mkdir()
            suffix = ".exe" if platform == "windows" else ""
            executable = deps / ("cedar_app-123abc" + suffix)
            executable.write_bytes(b"synthetic, never executed")
            for name in ("cedar-agent", "cedar-agent-interrupted-save-validation"):
                if name != missing_agent:
                    (release / (name + suffix)).write_bytes(b"synthetic, never executed")
            calls = []

            def execute(command, cwd, environment, timeout, limit):
                calls.append((command, timeout, limit))
                self.assertEqual(cwd, root)
                self.assertEqual(environment["CARGO_TARGET_DIR"], str(root / "target"))
                self.assertEqual(environment["CEDAR_SAVE_CLOSE_AGENT_BIN"], str(release / ("cedar-agent" + suffix)))
                self.assertEqual(environment["CEDAR_INTERRUPTED_SAVE_AGENT_BIN"],
                                 str(release / ("cedar-agent-interrupted-save-validation" + suffix)))
                if command[0] == "cargo":
                    stage = "compile"
                    self.assertEqual(command, proof.test_compile_command())
                    data = json.dumps(artifact(root, executable)).encode()
                    if invalid_artifact:
                        data = b"{}"
                elif "--list" in command:
                    stage = "selection"
                    self.assertEqual(command, [str(executable), proof.TEST_NAME, "--ignored", "--exact", "--list"])
                    data = listing() if selection is None else selection
                else:
                    stage = "runtime"
                    self.assertEqual(command, [str(executable), proof.TEST_NAME, "--ignored", "--exact",
                                               "--test-threads=1", "--nocapture"])
                    data = payload(platform=platform) if runtime is None else runtime
                if stage == stage_failure:
                    raise proof.AcceptanceError("subprocess_timeout")
                return data

            capture = io.StringIO()
            with mock.patch.object(proof.sys, "platform", "win32" if platform == "windows" else platform), \
                    mock.patch.object(proof, "bounded_process", side_effect=execute), \
                    mock.patch.dict(proof.os.environ, {"CEDAR_SAVE_CLOSE_AGENT_BIN": "wrong",
                                                      "CEDAR_INTERRUPTED_SAVE_AGENT_BIN": "wrong",
                                                      "CARGO_TARGET_DIR": "wrong"}), redirect_stdout(capture):
                receipts, error = None, None
                try:
                    receipts = proof.run(root)
                except proof.AcceptanceError as caught:
                    error = str(caught)
            return receipts, error, calls, capture.getvalue()

    def test_one_runtime_after_compile_and_exact_selection_both_platforms(self):
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                receipts, error, calls, output = self.exercise(platform)
                self.assertIsNone(error)
                self.assertEqual(receipts, proof.expected_receipts(platform))
                self.assertEqual([timeout for _, timeout, _ in calls], [120, 15, 180])
                self.assertEqual([limit for _, _, limit in calls],
                                 [4 * 1024 * 1024, 16 * 1024, 256 * 1024])
                self.assertEqual(output.count(proof.PREFIX), 6)
                self.assertIn("27 requests, 6 agents reaped", output)

    def test_preparation_failure_never_starts_runtime(self):
        for changes, expected_calls in (({"missing_agent": "cedar-agent"}, 0),
                                        ({"missing_agent": "cedar-agent-interrupted-save-validation"}, 0),
                                        ({"stage_failure": "compile"}, 1),
                                        ({"invalid_artifact": True}, 1),
                                        ({"stage_failure": "selection"}, 2),
                                        ({"selection": b"0 tests, 0 benchmarks\n"}, 2)):
            with self.subTest(changes=changes):
                receipts, error, calls, output = self.exercise(**changes)
                self.assertIsNone(receipts)
                self.assertIsNotNone(error)
                self.assertEqual(len(calls), expected_calls)
                self.assertEqual(output, "")

    def test_runtime_failure_or_invalid_receipts_never_retried_or_printed(self):
        for changes in ({"stage_failure": "runtime"}, {"runtime": b""},
                        {"runtime": payload() * 2}, {"runtime": payload(platform="windows")}):
            with self.subTest(changes=changes):
                receipts, error, calls, output = self.exercise(**changes)
                self.assertIsNone(receipts)
                self.assertIsNotNone(error)
                self.assertEqual(len(calls), 3)
                self.assertEqual(output, "")

    def test_unsupported_platform_starts_no_children(self):
        receipts, error, calls, output = self.exercise("darwin")
        self.assertIsNone(receipts)
        self.assertEqual(error, "unsupported_platform")
        self.assertEqual(calls, [])
        self.assertEqual(output, "")

    def test_main_rejects_arguments_and_sanitizes_driver_errors(self):
        with mock.patch.object(proof.sys, "argv", ["driver", "unexpected"]), \
                mock.patch.object(proof, "run") as run, redirect_stderr(io.StringIO()) as error:
            self.assertEqual(proof.main(), 1)
        run.assert_not_called()
        self.assertEqual(error.getvalue(), "Save and close acceptance failed: unexpected_arguments\n")
        for failure in (OSError("private fixture path"), ValueError("raw child content"),
                        proof.AcceptanceError("subprocess_timeout")):
            with self.subTest(failure=type(failure)), \
                    mock.patch.object(proof.sys, "argv", ["driver"]), \
                    mock.patch.object(proof, "run", side_effect=failure) as run, \
                    redirect_stderr(io.StringIO()) as error:
                self.assertEqual(proof.main(), 1)
            run.assert_called_once()
            category = "subprocess_timeout" if isinstance(failure, proof.AcceptanceError) else "driver_error"
            self.assertEqual(error.getvalue(), "Save and close acceptance failed: " + category + "\n")


if __name__ == "__main__":
    unittest.main()
