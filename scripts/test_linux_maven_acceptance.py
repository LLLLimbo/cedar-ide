#!/usr/bin/env python3
"""Linux Maven predicate and mocked driver tests. No Java, Rust, JDT or downloads."""
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock
from contextlib import redirect_stdout

import linux_maven_acceptance as acceptance


def stop():
    return {"platform": "linux", "status": "graceful", "reason": "root_exited",
            "root_exit": {"kind": "code", "code": 0}, "cleanup_joined": True,
            "shutdown_response_received": True, "exit_frame_completed": True}


def case(name):
    present = name == "present"
    return {
        **acceptance.CASE_FIXED,
        **{key: True for key in acceptance.CASE_TRUE},
        **{key: False for key in acceptance.CASE_FALSE},
        **{key: present for key in acceptance.CASE_PRESENT},
        **{key: low for key, (low, _) in acceptance.CASE_COUNTS.items()},
        **acceptance.EVENT_SUCCESS,
        "case": name, "failure_stage": "none",
        "model_status": "imported" if present else "unresolved",
        "dependency_observation": "present" if present else "absent",
        "offline_pom_diagnostic": not present,
        "project_missing_library_diagnostic": not present,
        "elapsed_ms": 1000, "stop": stop(),
    }


def valid():
    return {**acceptance.TOP_FIXED, **acceptance.TOP_ENUMS,
            **{key: True for key in acceptance.TOP_TRUE},
            **{key: False for key in acceptance.TOP_FALSE},
            "elapsed_ms": 3000, "cases": [case("present"), case("missing")]}


class ProbeTests(unittest.TestCase):
    def test_present_and_both_supported_missing_observations(self):
        for observation in ("absent", "not_observed"):
            receipt = valid()
            receipt["cases"][1].update(dependency_observation=observation,
                                        project_missing_library_diagnostic=True)
            before = copy.deepcopy(receipt)
            data = b"running 1 test\n" + json.dumps(receipt).encode() + b"\ntest result: ok\n"
            self.assertEqual(acceptance.parse_probe(data), before)
            self.assertEqual(receipt, before)

    def test_missing_case_cannot_forge_success_without_owned_project_marker(self):
        for observation in ("absent", "not_observed"):
            receipt = valid()
            receipt["cases"][1].update(dependency_observation=observation,
                                        project_missing_library_diagnostic=False)
            encoded = json.dumps(receipt).encode()
            self.assertEqual(acceptance.decode_probe(encoded), receipt)
            with self.assertRaisesRegex(ValueError, "missing-project"):
                acceptance.parse_probe(encoded)

    def test_project_marker_requires_exact_case_boolean_and_cannot_be_omitted(self):
        field = "project_missing_library_diagnostic"
        for index in (0, 1):
            for invalid in (index == 0, None, "true", "false", 0, 1):
                receipt = valid()
                receipt["cases"][index][field] = invalid
                with self.subTest(case=index, value=invalid), self.assertRaises(ValueError):
                    acceptance.parse_probe(json.dumps(receipt).encode())
            receipt = valid()
            del receipt["cases"][index][field]
            with self.subTest(case=index, absent=True), self.assertRaises(ValueError):
                acceptance.parse_probe(json.dumps(receipt).encode())

    def test_all_boolean_witnesses_reject_coercion(self):
        for index in (None, 0, 1):
            flags = acceptance.TOP_TRUE + acceptance.TOP_FALSE if index is None else acceptance.CASE_FLAGS
            for key in flags:
                for invalid in (None, 1, 0, "true", [], {}):
                    receipt = valid()
                    target = receipt if index is None else receipt["cases"][index]
                    target[key] = invalid
                    with self.subTest(case=index, key=key, value=invalid), self.assertRaises(ValueError):
                        acceptance.validate_probe(receipt)

    def test_all_required_boolean_verdicts_are_enforced(self):
        for index in (None, 0, 1):
            flags = acceptance.TOP_TRUE + acceptance.TOP_FALSE if index is None else (
                acceptance.CASE_TRUE + acceptance.CASE_FALSE + acceptance.CASE_PRESENT
                + ("offline_pom_diagnostic", "project_missing_library_diagnostic"))
            for key in flags:
                receipt = valid()
                target = receipt if index is None else receipt["cases"][index]
                target[key] = not target[key]
                with self.subTest(case=index, key=key), self.assertRaises(ValueError):
                    acceptance.validate_probe(receipt)
        receipt = valid()
        receipt["cases"][0]["project_missing_library_diagnostic"] = True
        with self.assertRaises(ValueError):
            acceptance.validate_probe(receipt)

    def test_closed_schema_at_each_level_and_exact_case_order(self):
        for index in (None, 0, 1):
            keys = acceptance.TOP_KEYS if index is None else acceptance.CASE_KEYS
            for key in keys:
                receipt = valid()
                target = receipt if index is None else receipt["cases"][index]
                del target[key]
                with self.subTest(case=index, key=key), self.assertRaises(ValueError):
                    acceptance.validate_probe(receipt)
            receipt = valid()
            target = receipt if index is None else receipt["cases"][index]
            target["raw_private"] = "SECRET_SENTINEL"
            with self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)
        for value in (None, [], True, "success", [valid()]):
            with self.assertRaises(ValueError):
                acceptance.validate_probe(value)
        for cases in ([], None, [case("present")], [case("present")] * 2,
                      [case("missing"), case("present")], [case("present"), case("missing"), case("missing")],
                      (case("present"), case("missing")), [None, case("missing")]):
            with self.subTest(cases=cases), self.assertRaises(ValueError):
                acceptance.validate_probe({**valid(), "cases": cases})

    def test_exact_identities_and_deadlines_reject_numeric_coercion(self):
        for index in (None, 0, 1):
            fixed = acceptance.TOP_FIXED if index is None else acceptance.CASE_FIXED
            for key, expected in fixed.items():
                for invalid in (True, False, float(expected), str(expected), expected + 1, expected - 1, None):
                    receipt = valid()
                    target = receipt if index is None else receipt["cases"][index]
                    target[key] = invalid
                    with self.subTest(case=index, key=key, value=invalid), self.assertRaises(ValueError):
                        acceptance.validate_probe(receipt)
        for key in acceptance.TOP_ENUMS:
            for invalid in (None, "private sentinel", True, 1, [], {}):
                with self.subTest(key=key, value=invalid), self.assertRaises(ValueError):
                    acceptance.validate_probe({**valid(), key: invalid})
        for index in (0, 1):
            for key in acceptance.CASE_ENUMS:
                for invalid in (None, "private sentinel", True, 1, [], {}):
                    receipt = valid()
                    receipt["cases"][index][key] = invalid
                    with self.subTest(case=index, key=key, value=invalid), self.assertRaises(ValueError):
                        acceptance.validate_probe(receipt)

    def test_counts_and_phase_durations_enforce_success_limits(self):
        for index in (0, 1):
            for key, (low, high) in acceptance.CASE_COUNTS.items():
                for invalid in (low - 1, high + 1, True, 1.0, "1", None, float("nan"), float("inf")):
                    receipt = valid()
                    receipt["cases"][index][key] = invalid
                    with self.subTest(case=index, key=key, value=invalid), self.assertRaises(ValueError):
                        acceptance.validate_probe(receipt)
            receipt = valid()
            receipt["cases"][index]["elapsed_ms"] = 480000
            receipt["elapsed_ms"] = 500000
            with self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)
        for invalid in (-1, True, 1.0, "3000", None, 960000, 2000 - 1):
            with self.subTest(elapsed=invalid), self.assertRaises(ValueError):
                acceptance.validate_probe({**valid(), "elapsed_ms": invalid})

    def test_marker_mask_matches_exact_count_and_generated_metadata(self):
        for count, mask, generated in ((0, 0, 0), (1, 1, 1), (2, 5, 3), (6, 63, 6)):
            receipt = valid()
            receipt["cases"][0].update(lifecycle_metadata_files=count,
                                         lifecycle_metadata_mask=mask, generated_metadata_files=generated)
            self.assertEqual(acceptance.validate_probe(receipt), receipt)
        for count, mask, generated in ((1, 0, 1), (1, 3, 3), (6, 63, 5), (0, 64, 128), (7, 63, 128)):
            receipt = valid()
            receipt["cases"][1].update(lifecycle_metadata_files=count,
                                         lifecycle_metadata_mask=mask, generated_metadata_files=generated)
            with self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)

    def test_model_and_observation_must_match_each_case(self):
        for index, key, value in ((0, "model_status", "unresolved"), (1, "model_status", "imported"),
                                  (0, "dependency_observation", "absent"), (0, "dependency_observation", "not_observed"),
                                  (1, "dependency_observation", "present"), (1, "dependency_observation", "none"),
                                  (0, "failure_stage", "model")):
            receipt = valid()
            receipt["cases"][index][key] = value
            with self.subTest(case=index, key=key, value=value), self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)

    def test_linux_stop_is_required_and_reuses_platform_owned_validation(self):
        for index in (0, 1):
            for invalid in (None, {**stop(), "platform": "windows"}, {**stop(), "cleanup_joined": False},
                            {**stop(), "raw_private": "SECRET_SENTINEL"},
                            {**stop(), "root_exit": {"kind": "code", "code": True}},
                            {**stop(), "root_exit": {"kind": "unobserved"}},
                            {**stop(), "status": "error"}, {**stop(), "exit_frame_completed": False}):
                receipt = valid()
                receipt["cases"][index]["stop"] = invalid
                with self.subTest(case=index, stop=invalid), self.assertRaises(ValueError):
                    acceptance.validate_probe(receipt)
            receipt = valid()
            receipt["cases"][index]["stop"].update(status="forced", reason="grace_expired",
                                                      root_exit={"kind": "signal", "signal": 9},
                                                      shutdown_response_received=False, exit_frame_completed=False)
            self.assertEqual(acceptance.validate_probe(receipt), receipt)

    def test_one_standalone_strict_json_receipt_and_no_private_substring_scan(self):
        encoded = json.dumps(valid()).encode()
        prefix = ("test " + acceptance.TEST_NAME + " ... ").encode()
        self.assertEqual(acceptance.parse_probe(prefix + b"\n" + encoded + b"\n"), valid())
        for output in (b"", b"no receipt", prefix + encoded, b"private " + encoded,
                       encoded + b"\n" + encoded, encoded[:-1] + b', "success": true}',
                       b"{broken}", b"\xff", b"x" * (acceptance.MAX_LOG_BYTES + 1),
                       encoded.replace(b'"elapsed_ms": 3000', b'"elapsed_ms": NaN')):
            with self.subTest(size=len(output)), self.assertRaises((ValueError, UnicodeError)):
                acceptance.parse_probe(output)

    def test_failed_receipt_is_sanitized_without_erasing_failed_witnesses(self):
        receipt = valid()
        receipt.update(success=False, primary_failed=True, cache_input_files=0,
                       elapsed_saturated=True, elapsed_ms=960000)
        for item in receipt["cases"]:
            item.update(success=False, primary_failed=True, failure_stage="model", model_status="none",
                        stop=None, elapsed_ms=480000, elapsed_saturated=True, model_queries=0,
                        generated_metadata_files=129)
        encoded = json.dumps(receipt).encode()
        self.assertEqual(acceptance.decode_probe(encoded), receipt)
        with self.assertRaises(ValueError):
            acceptance.parse_probe(encoded)
        for index, key, value in ((None, "elapsed_ms", 960001), (None, "cache_input_files", 84),
                                  (0, "elapsed_ms", 480001), (0, "model_queries", 65536),
                                  (0, "lifecycle_metadata_mask", 256),
                                  (0, "generated_data_bytes", 0x10000000000000000),
                                  (1, "failure_stage", "raw private sentinel")):
            changed = copy.deepcopy(receipt)
            target = changed if index is None else changed["cases"][index]
            target[key] = value
            with self.subTest(case=index, key=key), self.assertRaises(ValueError):
                acceptance.decode_probe(json.dumps(changed).encode())

    def test_every_finite_event_branch_is_preserved_on_failure_and_cannot_pass_success(self):
        for index in (0, 1):
            for key, choices in acceptance.EVENT_ENUMS.items():
                for choice in choices:
                    receipt = valid()
                    receipt["cases"][index][key] = choice
                    self.assertEqual(acceptance.decode_probe(json.dumps(receipt).encode()), receipt)
                    if choice != acceptance.EVENT_SUCCESS[key]:
                        with self.subTest(case=index, field=key, branch=choice), self.assertRaises(ValueError):
                            acceptance.validate_probe(receipt)
                    receipt.update(success=False, primary_failed=True)
                    receipt["cases"][index].update(success=False, primary_failed=True, failure_stage="model")
                    self.assertEqual(acceptance.decode_probe(json.dumps(receipt).encode()), receipt)

    def test_rejection_trace_preserves_earlier_marker_witnesses_without_making_failure_success(self):
        receipt = valid()
        receipt.update(success=False, primary_failed=True)
        missing = receipt["cases"][1]
        missing.update(
            success=False, primary_failed=True, failure_stage="model",
            event_probe_outcome="events_rejected", event_rejection="unexpected_project_diagnostic",
            rejected_diagnostic_origin="owned_project_without_trailing_slash",
            rejected_diagnostic_code_shape="integer_invalid_classpath",
            rejected_diagnostic_severity="error", rejected_diagnostic_message_class="owned_missing_maven_library",
            rejected_diagnostic_source_java=True, rejected_diagnostic_zero_range=True,
            rejected_diagnostic_dependency_jar_absent=True, rejected_diagnostic_dependency_pom_absent=True,
            offline_pom_diagnostic=True, project_missing_library_diagnostic=True,
        )
        encoded = json.dumps(receipt).encode()
        self.assertEqual(acceptance.decode_probe(encoded), receipt)
        with self.assertRaises(ValueError):
            acceptance.parse_probe(encoded)
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "private.log"
            log.write_bytes(encoded)
            self.assertEqual(acceptance.failure_probe(log), ("available", receipt))

    def test_rejection_trace_rejects_type_coercion_and_private_payloads_even_on_failure(self):
        receipt = valid()
        receipt.update(success=False, primary_failed=True)
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "private.log"
            fields = dict.fromkeys(acceptance.EVENT_ENUMS)
            fields.update(dict.fromkeys(key for key in acceptance.CASE_FALSE if key.startswith("rejected_diagnostic_")))
            for key in fields:
                for invalid in (None, 0, 1, 1.0, [], {}, ["none"], "PRIVATE_RAW_SENTINEL"):
                    changed = copy.deepcopy(receipt)
                    changed["cases"][1][key] = invalid
                    encoded = json.dumps(changed).encode()
                    with self.subTest(field=key, value=invalid), self.assertRaises(ValueError):
                        acceptance.decode_probe(encoded)
                    log.write_bytes(encoded)
                    self.assertEqual(acceptance.failure_probe(log), ("malformed", None))
            for key in acceptance.EVENT_ENUMS:
                changed = copy.deepcopy(receipt)
                changed["cases"][1][key] = True
                with self.subTest(field=key), self.assertRaises(ValueError):
                    acceptance.decode_probe(json.dumps(changed).encode())

    def test_failure_categories_never_publish_raw_output(self):
        self.assertEqual(acceptance.failure_probe(None), ("not_run", None))
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "private.log"
            for data, expected in ((b"SECRET_SENTINEL", "unavailable"),
                                   (b"{SECRET_SENTINEL", "malformed"),
                                   (b'{"nested":' + b'[' * 2000 + b'0' + b']' * 2000 + b'}', "malformed"),
                                   (json.dumps({**valid(), "raw_private": "SECRET_SENTINEL"}).encode(), "malformed")):
                log.write_bytes(data)
                self.assertEqual(acceptance.failure_probe(log), (expected, None))
            receipt = valid()
            receipt["success"] = False
            log.write_text(json.dumps(receipt))
            self.assertEqual(acceptance.failure_probe(log), ("available", receipt))


class PreparationTests(unittest.TestCase):
    def test_explicit_archive_with_wrong_or_missing_bytes_never_falls_back_to_acquisition(self):
        with tempfile.TemporaryDirectory() as temporary, \
                mock.patch.object(acceptance.linux_java, "prepare") as prepare, \
                mock.patch.object(acceptance.linux_java.urllib.request, "urlopen") as download:
            root = Path(temporary)
            with self.assertRaises(OSError):
                acceptance.prepare_archive(root / "missing", root / "distribution")
            archive = root / "existing.tar.gz"
            archive.write_bytes(b"not the sealed official archive")
            with self.assertRaises(ValueError):
                acceptance.prepare_archive(archive, root / "distribution")
            prepare.assert_not_called()
            download.assert_not_called()

    def test_archive_omission_uses_only_existing_pinned_official_preparer(self):
        self.assertEqual(acceptance.linux_java.ARCHIVE_URL,
                         "https://download.eclipse.org/jdtls/milestones/1.61.0/jdt-language-server-1.61.0-202609031315.tar.gz")
        self.assertEqual(acceptance.linux_java.ARCHIVE_SHA256,
                         "338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64")
        with tempfile.TemporaryDirectory() as temporary, \
                mock.patch.object(acceptance.linux_java, "prepare") as prepare, \
                mock.patch.object(acceptance.linux_java.urllib.request, "urlopen") as download:
            root = Path(temporary)
            distribution = root / "distribution"
            acceptance.prepare_archive(None, distribution)
            prepare.assert_called_once_with(root / "jdtls-1.61.0.tar.gz", distribution)
            self.assertFalse((root / "jdtls-1.61.0.tar.gz").exists())
            download.assert_not_called()

    def test_archive_copy_and_disk_preflight_precede_shared_safe_extraction(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "existing.tar.gz"
            data = b"synthetic pinned archive bytes"
            archive.write_bytes(data)
            distribution = root / "private" / "distribution"
            distribution.parent.mkdir()
            with mock.patch.object(acceptance.linux_java, "ARCHIVE_SHA256", hashlib.sha256(data).hexdigest()), \
                    mock.patch.object(acceptance.linux_java, "prepare") as prepare, \
                    mock.patch.object(acceptance.shutil, "disk_usage", return_value=mock.Mock(
                        free=len(data) + acceptance.linux_java.DISK_RESERVE_BYTES)):
                acceptance.prepare_archive(archive, distribution)
                copied = distribution.parent / "jdtls-1.61.0.tar.gz"
                self.assertEqual(copied.read_bytes(), data)
                self.assertEqual(archive.read_bytes(), data)
                prepare.assert_called_once_with(copied, distribution)
            with mock.patch.object(acceptance.linux_java, "ARCHIVE_SHA256", hashlib.sha256(data).hexdigest()), \
                    mock.patch.object(acceptance.linux_java, "prepare") as prepare, \
                    mock.patch.object(acceptance.shutil, "disk_usage", return_value=mock.Mock(free=0)):
                with self.assertRaises(acceptance.linux_java.DiskPreflightError):
                    acceptance.prepare_archive(archive, distribution)
                prepare.assert_not_called()

    def test_frozen_cache_verification_uses_exact_existing_inventory_and_no_acquisition(self):
        entries = acceptance.maven_cache.load_manifest()
        self.assertEqual((len(entries), sum(entry["bytes"] for entry in entries)), (83, 4065288))
        with tempfile.TemporaryDirectory() as temporary, \
                mock.patch.object(acceptance.maven_cache, "verify_cache") as verify, \
                mock.patch.object(acceptance.maven_cache, "prepare") as acquire:
            root = Path(temporary)
            acceptance.verify_cache(root)
            verify.assert_called_once_with(root, entries)
            acquire.assert_not_called()

    def test_cache_symlink_ancestor_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary, \
                mock.patch.object(acceptance.maven_cache, "verify_cache") as verify:
            root = Path(temporary)
            (root / "real").mkdir()
            (root / "linked").symlink_to(root / "real", target_is_directory=True)
            with self.assertRaises(acceptance.maven_cache.CacheError):
                acceptance.verify_cache(root / "linked" / "cache")
            verify.assert_not_called()


class SelectionTests(unittest.TestCase):
    def test_exact_aggregate_test_name_is_uniquely_listed(self):
        self.assertEqual(acceptance.TEST_NAME,
                         "language_ui::real_java_tests::acceptance::linux_maven::real_linux_normal_agent_java_maven_acceptance")
        listing = acceptance.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n"
        acceptance.validate_test_listing(listing.encode())
        acceptance.validate_test_listing(listing.replace("\n", "\r\n").encode())
        for invalid in (b"", b"0 tests, 0 benchmarks\n", b"\xff", b"x" * (acceptance.MAX_LIST_BYTES + 1),
                        listing.replace("linux_maven", "windows_maven").encode(),
                        listing.replace("1 test", "2 tests").encode(),
                        (listing + acceptance.TEST_NAME + ": test\n").encode(),
                        (listing + "SECRET_SENTINEL\n").encode()):
            with self.assertRaises((ValueError, UnicodeError)):
                acceptance.validate_test_listing(invalid)


class CliTests(unittest.TestCase):
    def test_archive_is_optional_but_existing_cache_remains_required(self):
        arguments = ["linux_maven_acceptance.py", "--scratch-root", "scratch", "--java", "java"]
        with mock.patch.object(acceptance.sys, "argv", arguments + ["--cache", "cache"]), \
                mock.patch.object(acceptance.os, "umask"), \
                mock.patch.object(acceptance, "run") as run:
            acceptance.main()
            self.assertEqual(run.call_args.args[-2:], (None, "cache"))
        with mock.patch.object(acceptance.sys, "argv", arguments), \
                mock.patch.object(acceptance.os, "umask"), \
                mock.patch.object(acceptance, "run") as run:
            with self.assertRaisesRegex(ValueError, "Maven cache"):
                acceptance.main()
            run.assert_not_called()

    def test_internal_preparer_can_select_only_distribution_with_optional_existing_archive(self):
        for archive in (None, "existing"):
            arguments = ["linux_maven_acceptance.py", "--prepare-distribution", "distribution"]
            if archive is not None:
                arguments.extend(["--prepare-archive", archive])
            with mock.patch.object(acceptance.sys, "argv", arguments), \
                    mock.patch.object(acceptance.os, "umask"), \
                    mock.patch.object(acceptance, "prepare_archive") as prepare, \
                    mock.patch.object(acceptance, "run") as run:
                acceptance.main()
                prepare.assert_called_once_with(archive, "distribution")
                run.assert_not_called()


class DriverTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.scratch = self.root / "scratch"
        self.scratch.mkdir()
        self.java = self.root / "jdk" / "bin" / "java"
        self.java.parent.mkdir(parents=True)
        self.java.write_bytes(b"synthetic JDK, never executed")
        self.java.chmod(0o700)
        (self.java.parent.parent / "release").write_text('JAVA_VERSION="21.0.9"\n')
        self.agent = self.root / "target" / "release" / "cedar-agent"
        self.agent.parent.mkdir(parents=True)
        self.agent.write_bytes(b"synthetic prebuilt normal agent, never executed")
        self.agent.chmod(0o700)
        self.executable = self.root / "target" / "debug" / "deps" / "cedar_app-synthetic"
        self.executable.parent.mkdir(parents=True)
        self.executable.write_bytes(b"synthetic test harness, never executed")
        self.executable.chmod(0o700)
        self.archive = self.root / "sealed.tar.gz"
        self.cache = self.root / "cache"
        self.commands = []
        self.runtime_data = json.dumps(valid()).encode()
        self.fail_stage = None
        self.mutate_stage = None

    def process(self, command, cwd, environment, log, timeout):
        self.commands.append((command, cwd, environment.copy(), log, timeout))
        if log.name == "source-commit-private.log":
            data = b"a" * 40 + b"\n"
        elif log.name == "source-dirty-private.log":
            data = b" M owned-source\n"
        elif log.name == "compile-private.log":
            data = json.dumps({"reason": "compiler-artifact", "target": {"name": "cedar_app"},
                               "profile": {"test": True}, "executable": str(self.executable)}).encode()
        elif log.name == "selection-private.log":
            data = (acceptance.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n").encode()
        elif log.name == "acceptance-private.log":
            data = self.runtime_data
        else:
            data = b"PRIVATE_SENTINEL private subprocess text\n"
        log.write_bytes(data)
        if self.mutate_stage == log.name:
            self.agent.write_bytes(b"changed prebuilt normal agent")
        if self.fail_stage == log.name:
            raise ValueError("PRIVATE_SENTINEL exception with private path")

    def run_driver(self):
        output = io.StringIO()
        with mock.patch.object(acceptance.sys, "platform", "linux"), \
                mock.patch.object(acceptance.platform, "machine", return_value="x86_64"), \
                mock.patch.object(acceptance, "bounded_process", side_effect=self.process), \
                mock.patch.dict(os.environ, {key: "PRIVATE_ENV_SENTINEL" for key in acceptance.LAUNCHER_ENVIRONMENT_KEYS}), \
                redirect_stdout(output):
            try:
                result = acceptance.run(self.root, self.scratch, self.java, self.agent, self.archive, self.cache)
                error = None
            except RuntimeError as caught:
                result = None
                error = str(caught)
        self.assertNotIn("PRIVATE", output.getvalue())
        self.assertNotIn(str(self.root), output.getvalue())
        evidence = json.loads((self.scratch / acceptance.EVIDENCE_NAME).read_text())
        self.assertEqual(json.loads(output.getvalue()), evidence)
        return result, error, evidence

    def test_success_uses_separate_bounds_exact_test_and_preserves_agent_identity(self):
        original = self.agent.read_bytes()
        result, error, evidence = self.run_driver()
        self.assertIsNone(error)
        self.assertEqual(result, evidence)
        self.assertEqual(result["status"], "success")
        self.assertEqual(result["agent_sha256"], hashlib.sha256(original).hexdigest())
        self.assertEqual(result["agent_build_provenance"], "caller_supplied_prebuilt")
        self.assertFalse(result["agent_source_equivalence_verified"])
        self.assertEqual(result["source_commit"], "a" * 40)
        self.assertTrue(result["checkout_dirty"])
        self.assertEqual([call[4] for call in self.commands], [10, 10, 180, 240, 15, 600, 15, 1020])
        self.assertEqual(self.commands[5][0], acceptance.linux_java.test_compile_command())
        self.assertEqual(self.commands[6][0], [str(self.executable), "--list", "--ignored", "--exact", acceptance.TEST_NAME])
        self.assertEqual(self.commands[7][0], [str(self.executable), "--ignored", "--exact", acceptance.TEST_NAME,
                                              "--nocapture", "--test-threads=1"])
        self.assertIn(str(self.archive), self.commands[2][0])
        self.assertEqual(self.commands[3][0][-2:], ["--verify-cache", str(self.cache)])
        for _, _, environment, _, _ in self.commands:
            self.assertTrue(all(key not in environment for key in acceptance.LAUNCHER_ENVIRONMENT_KEYS))
        environment = self.commands[-1][2]
        self.assertEqual(environment["CEDAR_AGENT_BIN"], str(self.agent))
        self.assertEqual(environment["CEDAR_MAVEN_CACHE_INPUT"], str(self.cache))
        self.assertFalse(Path(environment["TMPDIR"]).exists())
        for key in ("network_isolation_verified", "maven_goals_exercised", "dependency_source_navigation_exercised",
                    "raw_logs_published", "gui_exercised"):
            self.assertFalse(result[key])
        self.assertEqual(result["preparation_network_requests"], 0)
        self.assertTrue(result["existing_archive_reused"])
        self.assertTrue(result["existing_cache_reused"])
        self.assertFalse(result["archive_acquisition_performed"])
        self.assertEqual(result["launcher_environment_keys_checked"], 11)
        self.assertEqual(list(self.scratch.iterdir()), [self.scratch / acceptance.EVIDENCE_NAME])

    def test_archive_acquisition_stays_in_180_second_preparation_and_reports_no_exact_http_count(self):
        self.archive = None
        result, error, _ = self.run_driver()
        self.assertIsNone(error)
        self.assertFalse(result["existing_archive_reused"])
        self.assertTrue(result["archive_acquisition_performed"])
        self.assertTrue(result["existing_cache_reused"])
        self.assertNotIn("preparation_network_requests", result)
        command, _, _, _, timeout = self.commands[2]
        self.assertEqual(timeout, 180)
        self.assertIn("--prepare-distribution", command)
        self.assertNotIn("--prepare-archive", command)
        self.assertNotIn("None", command)
        self.assertEqual(self.commands[3][0][-2:], ["--verify-cache", str(self.cache)])

    def test_preparation_failure_never_compiles_and_retains_private_artifacts(self):
        self.fail_stage = "cache-verification-private.log"
        result, error, evidence = self.run_driver()
        self.assertIsNone(result)
        self.assertNotIn("PRIVATE", error)
        self.assertEqual(evidence["stage"], "cache_verification")
        self.assertTrue(evidence["private_scratch_retained"])
        self.assertEqual(evidence["probe_record_status"], "not_run")
        self.assertEqual(len(self.commands), 4)
        self.assertTrue(self.commands[-1][3].exists())

    def test_compile_agent_mutation_prevents_selection_and_acceptance(self):
        self.mutate_stage = "compile-private.log"
        _, _, evidence = self.run_driver()
        self.assertEqual(evidence["stage"], "compile")
        self.assertEqual(len(self.commands), 6)
        self.assertTrue(evidence["private_scratch_retained"])

    def test_runtime_agent_mutation_cannot_pass_and_does_not_discard_raw_receipt(self):
        self.mutate_stage = "acceptance-private.log"
        _, _, evidence = self.run_driver()
        self.assertEqual(evidence["stage"], "acceptance")
        self.assertEqual(evidence["probe_record_status"], "available")
        self.assertTrue(evidence["private_scratch_retained"])
        self.assertTrue(self.commands[-1][3].exists())

    def test_failed_process_preserves_strict_failed_receipt(self):
        failed = valid()
        failed.update(success=False, primary_failed=True)
        failed["cases"][0].update(success=False, primary_failed=True, failure_stage="semantics",
                                  event_probe_outcome="events_rejected", event_rejection="unexpected_source_diagnostic",
                                  rejected_diagnostic_origin="source", rejected_diagnostic_code_shape="other_integer",
                                  rejected_diagnostic_message_class="other", rejected_diagnostic_source_java=True)
        self.runtime_data = json.dumps(failed).encode()
        self.fail_stage = "acceptance-private.log"
        _, _, evidence = self.run_driver()
        self.assertEqual(evidence["status"], "failed")
        self.assertEqual(evidence["probe"], failed)
        self.assertEqual(evidence["probe_record_status"], "available")

    def test_malformed_receipt_never_escapes_private_log(self):
        self.runtime_data = json.dumps({**valid(), "raw_private": "PRIVATE_SENTINEL"}).encode()
        _, _, evidence = self.run_driver()
        self.assertEqual(evidence["probe_record_status"], "malformed")
        self.assertNotIn("probe", evidence)
        self.assertEqual(self.commands[-1][3].read_bytes(), self.runtime_data)

    def test_non_linux_or_missing_cache_fails_before_any_paths_or_processes(self):
        for host, machine, archive, cache in (("win32", "x86_64", "archive", "cache"),
                                             ("linux", "aarch64", "archive", "cache"),
                                             ("linux", "x86_64", "archive", None)):
            with mock.patch.object(acceptance.sys, "platform", host), \
                    mock.patch.object(acceptance.platform, "machine", return_value=machine), \
                    mock.patch.object(acceptance.Path, "resolve") as resolve, \
                    mock.patch.object(acceptance, "bounded_process") as launch:
                with self.assertRaises(ValueError):
                    acceptance.run("unused", "unused", "unused", "unused", archive, cache)
                resolve.assert_not_called()
                launch.assert_not_called()

    def test_non_ascii_scratch_or_java_fails_before_any_processes(self):
        unicode_scratch = self.root / "scratch 雪"
        unicode_scratch.mkdir()
        unicode_java = self.root / "jdk 雪" / "bin" / "java"
        unicode_java.parent.mkdir(parents=True)
        unicode_java.write_bytes(b"never executed")
        unicode_java.chmod(0o700)
        for scratch, java in ((unicode_scratch, self.java), (self.scratch, unicode_java)):
            with mock.patch.object(acceptance.sys, "platform", "linux"), \
                    mock.patch.object(acceptance.platform, "machine", return_value="x86_64"), \
                    mock.patch.object(acceptance, "bounded_process") as launch:
                with self.assertRaisesRegex(ValueError, "ASCII"):
                    acceptance.run(self.root, scratch, java, self.agent, self.archive, self.cache)
                launch.assert_not_called()

    def test_existing_or_broken_symlink_evidence_destination_fails_before_execution(self):
        evidence = self.scratch / acceptance.EVIDENCE_NAME
        evidence.symlink_to(self.scratch / "absent")
        with mock.patch.object(acceptance.sys, "platform", "linux"), \
                mock.patch.object(acceptance.platform, "machine", return_value="x86_64"), \
                mock.patch.object(acceptance, "bounded_process") as launch:
            with self.assertRaisesRegex(ValueError, "fresh"):
                acceptance.run(self.root, self.scratch, self.java, self.agent, self.archive, self.cache)
            launch.assert_not_called()


if __name__ == "__main__":
    unittest.main()
