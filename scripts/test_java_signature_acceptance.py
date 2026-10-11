#!/usr/bin/env python3
"""Pure contracts, synthetic archives and tiny Python supervision tests only.

Never compiles Rust, starts an agent/JVM, fetches JDT, or changes old workflows.
"""
import copy
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import java_signature_acceptance as proof
from test_linux_agent_bundle import synthetic_elf
from test_windows_bundle import synthetic_pe

COMMIT = "a" * 40
RUN_URL = proof.windows_bundle.REPOSITORY + "/actions/runs/123456789"


def good_probe(platform="linux"):
    false = {"primary_failed", "cleanup_failed", "fixture_retained", "elapsed_saturated"}
    value = {key: key not in false for key in proof.PROBE_FLAGS}
    value.update(proof.PROBE_FIXED)
    value.update({key: 0 for key in proof.PROBE_COUNTS})
    value.update(kind="java_parameter_info_normal_agent", platform=platform,
                 route="normal_agent_normal_client", status="passed", failure_stage="none",
                 capability_count=32, hello_count=1, list_count=1, read_count=1, begin_count=1,
                 open_count=1, close_count=1, stop_count=1, poll_count=2, signature_count=3,
                 first_result_count=2, second_result_count=2, active_elapsed_ms=1000,
                 cleanup_elapsed_ms=2000, elapsed_ms=3000, source_fixture_sha256=proof.SOURCE_FIXTURE_SHA256)
    return value


def encoded(value):
    return json.dumps(value).encode()


def listing():
    return (proof.TEST_NAME + ": test\n\n1 test, 0 benchmarks\n").encode()


def executed():
    return ("\nrunning 1 test\ntest " + proof.TEST_NAME + " ... ok\n\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 999 filtered out; finished in 0.01s\n").encode()


def package_fixture(base, platform="linux"):
    root, artifact, verified = base / "source", base / "download", base / "verified"
    root.mkdir()
    artifact.mkdir()
    module = proof.package_module(platform)
    payload = {target: (source + "\n").encode() for source, target in module.SOURCE_FILES.items()}
    payload["third-party-licenses/example-1.0.0/LICENSE-MIT"] = b"A fixture notice\n"
    reverse = {target: source for source, target in module.SOURCE_FILES.items()}
    for name, content in payload.items():
        path = root / reverse.get(name, name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    (root / "Cargo.toml").write_bytes(b'[workspace.package]\nversion = "0.59.0"\n')
    payload["cedar-agent" if platform == "linux" else "cedar-agent.exe"] = synthetic_elf() if platform == "linux" else synthetic_pe()
    if platform == "windows":
        payload["cedar.exe"] = synthetic_pe()
    manifest = module.make_manifest("0.59.0", COMMIT, RUN_URL, payload)
    archive = module.archive_bytes(payload, manifest)
    (artifact / ("generated.tar.gz" if platform == "linux" else "generated.zip")).write_bytes(archive)
    return root, artifact, verified, module, payload, manifest


class ReceiptTests(unittest.TestCase):
    def test_exact_success_and_typed_partial_records(self):
        for platform in ("linux", "windows"):
            self.assertEqual(proof.validate_probe(encoded(good_probe(platform)), platform, successful=True), good_probe(platform))
        failed = good_probe()
        failed.update(status="failed", failure_stage="second_argument", primary_failed=True,
                      second_argument_active_exact=False, signature_count=2)
        self.assertEqual(proof.validate_probe(encoded(failed), "linux", successful=False), failed)
        with self.assertRaises(ValueError):
            proof.validate_probe(encoded(failed), "linux", successful=True)
        failed.update(failure_stage="outside_call", outside_result_count=1)
        self.assertEqual(proof.validate_probe(encoded(failed), "linux", successful=False)["outside_result_count"], 1)

    def test_missing_extra_wrong_types_and_each_success_witness(self):
        value = good_probe()
        for key in value:
            with self.subTest(missing=key), self.assertRaises(ValueError):
                proof.validate_probe(encoded({name: item for name, item in value.items() if name != key}), "linux", successful=True)
            if key in proof.PROBE_FLAGS:
                for bad in (0, 1, "true", not value[key]):
                    with self.subTest(key=key, bad=bad), self.assertRaises(ValueError):
                        proof.validate_probe(encoded({**value, key: bad}), "linux", successful=True)
            if key in proof.PROBE_FIXED or key in proof.PROBE_COUNTS:
                for bad in (True, "1", 1.0, -1):
                    with self.subTest(key=key, bad=bad), self.assertRaises(ValueError):
                        proof.validate_probe(encoded({**value, key: bad}), "linux", successful=False)
        with self.assertRaises(ValueError):
            proof.validate_probe(encoded({**value, "private_path": "secret"}), "linux", successful=False)

    def test_caps_fixed_time_bounds_hash_platform_and_nonfinite(self):
        value = good_probe()
        for key, cap in proof.PROBE_COUNTS.items():
            with self.subTest(key=key), self.assertRaises(ValueError):
                proof.validate_probe(encoded({**value, key: cap + 1}), "linux", successful=False)
        for key in proof.PROBE_FIXED:
            with self.subTest(key=key), self.assertRaises(ValueError):
                proof.validate_probe(encoded({**value, key: value[key] + 1}), "linux", successful=False)
        for field, bad in (("platform", "macos"), ("source_fixture_sha256", "a" * 63),
                           ("failure_stage", "/private/source.java"), ("elapsed_ms", 2999)):
            with self.assertRaises(ValueError):
                proof.validate_probe(encoded({**value, field: bad}), "linux", successful=False)
        for data in (b'{"a":1,"a":2}', b'{"a":NaN}', b'\xff', b"x" * (proof.MAX_RECEIPT_BYTES + 1), encoded(value) + encoded(value)):
            with self.assertRaises(ValueError):
                proof.validate_probe(data, "linux", successful=False)

    def test_selection_and_execution_require_one_exact_ignored_case(self):
        proof.validate_listing(listing())
        proof.validate_listing(listing().replace(b"\n", b"\r\n"))
        proof.validate_execution(executed())
        for data in (b"", listing() + listing(), listing().replace(proof.TEST_NAME.encode(), b"other"),
                     listing() + b"private text\n", b"x" * (proof.MAX_LIST_BYTES + 1)):
            with self.assertRaises(ValueError):
                proof.validate_listing(data)
        for data in (executed().replace(b"running 1", b"running 0"), executed().replace(b"1 passed", b"0 passed"),
                     executed().replace(b" ... ok", b" ... ignored"), executed() + executed()):
            with self.assertRaises(ValueError):
                proof.validate_execution(data)


class PackageTests(unittest.TestCase):
    def test_real_validators_exact_run_commit_and_mapped_sources_on_both_os(self):
        for platform in ("linux", "windows"):
            with tempfile.TemporaryDirectory() as temporary:
                root, artifact, verified, module, payload, _ = package_fixture(Path(temporary), platform)
                result = proof.verify_artifact(root, artifact, verified, COMMIT, RUN_URL, platform)
                agent = verified / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
                self.assertEqual(agent.read_bytes(), payload[agent.name])
                self.assertEqual(result["agent_sha256"], proof.digest(agent, proof.MAX_AGENT_BYTES))
                self.assertEqual(proof.validate_provenance((verified / proof.PROVENANCE_NAME).read_bytes(), COMMIT, RUN_URL, platform), result)
                self.assertTrue((root / "docs/MAVEN_PROJECTS.md").exists())
                self.assertFalse((root / "MAVEN_PROJECTS.md").exists())
                for commit, run in (("b" * 40, RUN_URL), (COMMIT, RUN_URL + "9")):
                    with self.assertRaises(ValueError):
                        proof.verify_artifact(root, artifact, verified, commit, run, platform)

    def test_changed_current_source_or_duplicate_archive_fail_before_agent_copy(self):
        for platform in ("linux", "windows"):
            with tempfile.TemporaryDirectory() as temporary:
                root, artifact, verified, _, _, _ = package_fixture(Path(temporary), platform)
                (root / "docs/MAVEN_PROJECTS.md").write_text("private forged guide")
                with self.assertRaises(ValueError):
                    proof.verify_artifact(root, artifact, verified, COMMIT, RUN_URL, platform)
                self.assertFalse(verified.exists())
                archive = next(artifact.iterdir())
                (artifact / ("duplicate" + (".zip" if platform == "windows" else ".tar.gz"))).write_bytes(archive.read_bytes())
                with self.assertRaises(ValueError):
                    proof.verify_artifact(root, artifact, verified, COMMIT, RUN_URL, platform)

    def test_provenance_rejects_bool_integers_extra_keys_and_cross_run(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, artifact, verified, _, _, _ = package_fixture(Path(temporary))
            result = proof.verify_artifact(root, artifact, verified, COMMIT, RUN_URL, "linux")
            for key, value in (("schema_version", True), ("source_commit", "b" * 40),
                               ("ci_run_url", RUN_URL + "9"), ("artifact", "latest"),
                               ("agent_sha256", "A" * 64), ("package_verified", 1), ("extra", 1)):
                with self.subTest(key=key), self.assertRaises(ValueError):
                    proof.validate_provenance(encoded({**result, key: value}), COMMIT, RUN_URL, "linux")

    def test_paths_symlinks_cap_and_source_inventory_rejections(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "Cargo.toml"
            content = b"source\n"
            path.write_bytes(content)
            object_id = proof.windows_bundle.blob_hash(content)
            inventory = proof.source_inventory(f"100644 blob {object_id}\tCargo.toml\0".encode())
            self.assertEqual(proof.snapshot(root, inventory), {"Cargo.toml": proof.digest(path, 100)})
            path.write_bytes(b"different\n")
            with self.assertRaises(ValueError):
                proof.snapshot(root, inventory)
            with self.assertRaises(ValueError):
                proof.regular(path, 1)
            if os.name != "nt":
                link = root / "redirect"
                link.symlink_to(path)
                with self.assertRaises(ValueError):
                    proof.regular(link, 100)
            for name in ("../Cargo.toml", "/Cargo.toml", "C:\\Cargo.toml", "C:/Cargo.toml",
                         "a/Cargo.toml:stream", "a//Cargo.toml"):
                with self.assertRaises(ValueError):
                    proof.source_inventory(f"100644 blob {object_id}\t{name}\0".encode())
            for mode in ("120000", "160000"):
                with self.assertRaises(ValueError):
                    proof.source_inventory(f"{mode} blob {object_id}\tCargo.toml\0".encode())


class DriverTests(unittest.TestCase):
    def exercise(self, *, platform="linux", fail_at=None, mutate_source=False, partial=False):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root, artifact, verified, _, _, _ = package_fixture(base, platform)
            proof.verify_artifact(root, artifact, verified, COMMIT, RUN_URL, platform)
            java = base / "jdk"
            java.mkdir()
            (java / "release").write_text('JAVA_VERSION="21.0.8"\n')
            executable = root / "target/debug/deps" / ("cedar_app-abc.exe" if platform == "windows" else "cedar_app-abc")
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"mock harness, never executed")
            source = (root / "Cargo.toml").read_bytes()
            inventory = f"100644 blob {proof.windows_bundle.blob_hash(source)}\tCargo.toml\0".encode()
            calls = []
            def runner(command, cwd, environment, timeout, limit):
                calls.append((command, timeout, limit))
                ordinal = len(calls)
                if ordinal == fail_at:
                    if partial and ordinal == 7:
                        failed = good_probe(platform)
                        failed.update(status="failed", primary_failed=True, failure_stage="first_argument")
                        Path(environment["CEDAR_REAL_JDT_PARAMETER_INFO_RECEIPT"]).write_bytes(encoded(failed))
                    raise proof.ChildFailure("subprocess_timeout", output_bytes=limit, truncated=False, reaped=True)
                if ordinal == 1:
                    return COMMIT.encode() + b"\n"
                if ordinal == 2:
                    return inventory
                if ordinal in (3, 4):
                    return b""
                if ordinal == 5:
                    return encoded({"reason": "compiler-artifact", "manifest_path": str(root / "crates/app/Cargo.toml"),
                                    "target": {"name": "cedar_app", "kind": ["lib"], "src_path": str(root / "crates/app/src/lib.rs")},
                                    "profile": {"test": True}, "executable": str(executable)}) + b"\n"
                if ordinal == 6:
                    return listing()
                if ordinal == 7:
                    Path(environment["CEDAR_REAL_JDT_PARAMETER_INFO_RECEIPT"]).write_bytes(encoded(good_probe(platform)))
                    if mutate_source:
                        (root / "Cargo.toml").write_bytes(source + b"#changed\n")
                    return executed()
                self.fail("Unexpected additional child")
            with mock.patch.object(proof, "native_platform", return_value=platform), mock.patch.dict(os.environ, {"JAVA_HOME_21_X64": str(java)}):
                result = proof.run(root, artifact, verified, base, COMMIT, RUN_URL, runner=runner)
            public = (base / proof.EVIDENCE_NAME).read_bytes()
            self.assertNotIn(str(base).encode(), public)
            self.assertNotIn(b"mock harness", public)
            self.assertNotIn(b"private forged", public)
            self.assertFalse(result["native_gui_proved"])
            self.assertFalse(result["raw_output_published"])
            return result, calls

    def test_both_platforms_exact_stage_budgets_and_no_offline_or_bundled_workflow(self):
        for platform in ("linux", "windows"):
            result, calls = self.exercise(platform=platform)
            self.assertEqual(result["status"], "passed")
            self.assertEqual([call[1] for call in calls], [10, 10, 15, 180, 600, 15, 780])
            self.assertEqual(sum(call[1] for call in calls), 1610)
            command = calls[4][0]
            self.assertEqual(command, proof.compile_command())
            self.assertNotIn("--offline", command)
            self.assertNotIn("--release", command)
            self.assertEqual(calls[-1][0][1], proof.TEST_NAME)
            self.assertTrue(result["source_before_after_verified"])
            self.assertTrue(result["packaged_agent_unchanged"])

    def test_every_failed_stage_is_terminal_and_original_failure_preserved(self):
        stages = ["source_commit", "source_inventory", "jdk", "prepare", "compile", "selection", "runtime"]
        for ordinal, stage in enumerate(stages, 1):
            result, calls = self.exercise(fail_at=ordinal)
            self.assertEqual(len(calls), ordinal)
            self.assertEqual(result["stage"], stage)
            self.assertEqual(result["category"], "subprocess_timeout")
            self.assertTrue(result["partial_evidence"])
            self.assertFalse(result["source_before_after_verified"])
            self.assertTrue(result["private_scratch_retained"])
            self.assertFalse(result["child_failure"]["descendant_cleanup_inferred"])
            self.assertEqual((result["child_failure"]["timeout_s"], result["child_failure"]["output_limit_bytes"]), proof.CHILD_BOUNDS[stage])
            self.assertLessEqual(result["child_failure"]["output_bytes"], result["child_failure"]["output_limit_bytes"])

    def test_failed_native_record_is_retained_and_changed_source_cannot_pass(self):
        result, _ = self.exercise(fail_at=7, partial=True)
        self.assertEqual(result["probe_record_status"], "valid")
        self.assertEqual(result["probe"]["failure_stage"], "first_argument")
        self.assertEqual(result["category"], "subprocess_timeout")
        result, _ = self.exercise(mutate_source=True)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["stage"], "source_after")
        self.assertFalse(result["source_before_after_verified"])

    def test_finalization_budget_has_one_shared_clock(self):
        budget = proof.FinalizationBudget()
        with mock.patch.object(proof.time, "monotonic", side_effect=[0, 12, 12, 31]):
            self.assertEqual(budget.call(lambda: 1), 1)
            with self.assertRaisesRegex(ValueError, "finalization_timeout"):
                budget.call(lambda: 2)
        self.assertEqual(budget.used, 31)

    def test_publication_precheck_postcheck_and_failure_replacement_share_budget(self):
        original = {"status": "passed", "stage": "complete", "category": "none", "partial_evidence": False}
        for times in ([0, 31, 32, 33, 34], [0, 1, 31, 32, 33, 34], [0, 1, 2, 31, 32, 33]):
            with tempfile.TemporaryDirectory() as temporary:
                path = Path(temporary) / proof.EVIDENCE_NAME
                budget = proof.FinalizationBudget()
                with mock.patch.object(proof.time, "monotonic", side_effect=times):
                    result = proof.publish_final(path, original, budget)
                self.assertEqual(result["status"], "failed")
                self.assertEqual(result["category"], "finalization_timeout")
                self.assertEqual(result["stage"], "receipt")
                self.assertTrue(result["partial_evidence"])
                self.assertFalse(result["receipt_publication_verified"])
                self.assertEqual(json.loads(path.read_bytes()), result)
                self.assertGreater(budget.used, proof.FINALIZE_TIMEOUT)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / proof.EVIDENCE_NAME
            budget = proof.FinalizationBudget()
            budget.used = proof.FINALIZE_TIMEOUT
            failed = dict(original, status="failed", stage="runtime", category="subprocess_timeout")
            with mock.patch.object(proof.time, "monotonic", side_effect=[0, 1, 2, 3]):
                result = proof.publish_final(path, failed, budget)
            self.assertEqual(result["category"], "subprocess_timeout")
            self.assertEqual(result["stage"], "runtime")
            self.assertEqual(result["receipt_failure_category"], "finalization_timeout")
            self.assertFalse(path.with_name(path.name + ".pending").exists())

    def test_failed_replacement_withholds_an_already_published_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / proof.EVIDENCE_NAME
            budget = proof.FinalizationBudget()
            original = {"status": "passed", "stage": "complete", "category": "none", "partial_evidence": False}
            real_publish = proof.publish
            def writer(destination, value):
                if destination.name.endswith(".incomplete"):
                    raise OSError("private path")
                real_publish(destination, value)
            with mock.patch.object(proof.time, "monotonic", side_effect=[0, 1, 31, 32, 33, 34]), mock.patch.object(proof, "publish", side_effect=writer):
                with self.assertRaisesRegex(ValueError, "receipt_publication_unavailable"):
                    proof.publish_final(path, original, budget)
            self.assertFalse(path.exists())
            self.assertGreater(budget.used, proof.FINALIZE_TIMEOUT)

    def test_publication_io_error_emits_only_fixed_incomplete_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / proof.EVIDENCE_NAME
            original = {"status": "passed", "stage": "complete", "category": "none", "partial_evidence": False}
            real_publish = proof.publish
            def writer(destination, value):
                if destination.name.endswith(".pending"):
                    raise OSError("private credentials and paths")
                real_publish(destination, value)
            with mock.patch.object(proof, "publish", side_effect=writer):
                result = proof.publish_final(path, original, proof.FinalizationBudget())
            self.assertEqual(result["category"], "receipt_io_error")
            self.assertIn(result["category"], proof.CATEGORIES)
            self.assertEqual(json.loads(path.read_bytes()), result)
            self.assertNotIn(b"private", path.read_bytes())


class ProcessTests(unittest.TestCase):
    def test_short_python_child_success_nonzero_flood_and_timeout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def run(source, timeout=2, cap=128):
                return proof.bounded_process([sys.executable, "-B", "-c", source], root, dict(os.environ), timeout, cap)
            self.assertEqual(run('print("ready")'), b"ready\n")
            for source, timeout, cap, category in (
                ('print("secret path"); raise SystemExit(7)', 2, 128, "subprocess_nonzero"),
                ('print("x" * 100000)', 2, 128, "subprocess_output_limit"),
                ('import time; time.sleep(5)', 0.08, 128, "subprocess_timeout"),
            ):
                with self.subTest(category=category), self.assertRaises(proof.ChildFailure) as caught:
                    run(source, timeout, cap)
                self.assertEqual(caught.exception.category, category)
                self.assertTrue(caught.exception.reaped)
                self.assertLessEqual(caught.exception.output_bytes, cap)
                self.assertNotIn("secret", str(caught.exception))

    def test_cleanup_uncertainty_does_not_replace_primary_failure(self):
        process = mock.Mock(returncode=9)
        process.stdout.fileno.return_value = 1
        process.poll.return_value = 9
        process.wait.side_effect = subprocess.TimeoutExpired("private command", 20)
        with mock.patch.object(proof.subprocess, "Popen", return_value=process), mock.patch.object(proof.os, "set_blocking"), mock.patch.object(proof.os, "read", return_value=b""):
            with self.assertRaises(proof.ChildFailure) as caught:
                proof.bounded_process(["private"], Path.cwd(), {}, 1, 64)
        self.assertEqual(caught.exception.category, "subprocess_nonzero")
        self.assertFalse(caught.exception.reaped)
        process.kill.assert_not_called()
        process.wait.assert_called_once_with(timeout=20)


class WorkflowTests(unittest.TestCase):
    def test_native_schema_and_static_fixture_digest_match_the_driver(self):
        root = Path(__file__).resolve().parent.parent
        native = (root / "crates/app/src/java_parameter_info_acceptance_tests.rs").read_text()
        body = native.split("struct Evidence {", 1)[1].split("\n}", 1)[0]
        fields = set(re.findall(r"^    ([a-z0-9_]+):", body, re.MULTILINE))
        self.assertEqual(fields, set(good_probe()))
        source_literal = re.search(r'const SOURCE: &str = (".*");', native).group(1)
        self.assertEqual(proof.hashlib.sha256(json.loads(source_literal).encode()).hexdigest(), proof.SOURCE_FIXTURE_SHA256)
        self.assertIn('const SOURCE_SHA256: &str = "' + proof.SOURCE_FIXTURE_SHA256 + '";', native)
        self.assertIn("fn " + proof.TEST_NAME.split("::")[-1] + "()", native)
        self.assertIn('mod java_parameter_info_acceptance_tests;', (root / "crates/app/src/lib.rs").read_text())

    def test_independent_same_run_job_budget_and_old_java_clocks(self):
        workflow = (Path(__file__).resolve().parent.parent / ".github/workflows/ci.yml").read_text()
        new = workflow.split("\n  java_signature:\n", 1)[1]
        self.assertIn("needs: test", new)
        self.assertIn("timeout-minutes: 35", new)
        self.assertIn("timeout-minutes: 28", new)
        self.assertEqual(new.count("timeout-minutes: 1"), 6)
        self.assertIn("ref: ${{ github.sha }}", new)
        self.assertIn("artifact: cedar-linux-agent-development", new)
        self.assertIn("artifact: cedar-windows-development", new)
        self.assertIn("uses: actions/download-artifact@v4", new)
        self.assertNotIn("github-token:", new)
        self.assertNotIn("run-id:", new)
        self.assertIn("if: always()", new)
        self.assertIn("matrix.os == 'ubuntu-24.04' && 90 || 45", workflow)
        self.assertIn("Real Linux typed Java (normal shipping agent)\n        timeout-minutes: 28", workflow)
        self.assertIn("Real Windows Java transport and agent/editor acceptance\n        timeout-minutes: 12", workflow)
        self.assertEqual(60 + 120 + 60 + 60 + 1680 + 60, 2040)
        self.assertEqual(600 + 180 + 10 + 10 + 15 + 15 + 780 + 20 + 30 + 20, 1680)


if __name__ == "__main__":
    unittest.main()
