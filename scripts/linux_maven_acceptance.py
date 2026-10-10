#!/usr/bin/env python3
"""Run the exact native Linux Maven present/missing acceptance with private logs.

Requires an existing JDK 21, caller-supplied prebuilt normal cedar-agent, and
the frozen Maven cache. Prefer an existing pinned official JDT 1.61 archive;
if omitted, preparation acquires only the same pinned official archive.
Archive preparation, cache verification, compilation and acceptance have separate hard
process bounds. No Maven goals, dependency-source navigation, project builds,
GUI, SSH, deployment or network-isolation acceptance is implied.
Run as a standalone CLI with exclusive child-reaper ownership, as required by
the shared Linux Java subprocess supervisor.
"""
import argparse
import hashlib
import os
from pathlib import Path
import platform
import re
import shutil
import sys
import tempfile

import linux_java_acceptance as linux_java
import prepare_maven_cache as maven_cache


EVIDENCE_NAME = "LINUX_MAVEN_ACCEPTANCE.json"
TEST_NAME = "language_ui::real_java_tests::acceptance::linux_maven::real_linux_normal_agent_java_maven_acceptance"
PREPARE_TIMEOUT = 180
CACHE_VERIFY_TIMEOUT = 240
COMPILE_TIMEOUT = 600
TEST_TIMEOUT = 1020
SELECTION_TIMEOUT = 15
MAX_LOG_BYTES = linux_java.MAX_LOG_BYTES
MAX_LIST_BYTES = linux_java.MAX_LIST_BYTES
LAUNCHER_ENVIRONMENT_KEYS = linux_java.JAVA_ENVIRONMENT_KEYS + (
    "MAVEN_OPTS", "MAVEN_ARGS", "MAVEN_CONFIG", "M2_HOME", "MAVEN_HOME",
)
require = linux_java.require
strict_json = linux_java.strict_json
read_regular = linux_java.read_regular
bounded_process = linux_java.bounded_process
compiled_test = linux_java.compiled_test

TOP_FIXED = {"schema_version": 1, "pair_deadline_ms": 960_000, "cache_input_files": 83}
TOP_ENUMS = {
    "kind": "linux_java_maven", "route": "normal_agent_normal_client",
    "source_sha256": "5dda0de22c2184b1420be8e68f8a37e9165b59658d5c5cbf9fe2ee770a1003e5",
    "pom_sha256": "c13116c2a4d7dd73f28f604480b6aad3ce818a11db526b7f61737c1c3864b65b",
    "dependency_jar_sha256": "82579c654968c77f0bd3d04c28a22b24396c35270ce76d015807410438952b5d",
}
TOP_TRUE = ("cache_input_unchanged", "fixture_inputs_verified", "success")
TOP_FALSE = ("elapsed_saturated", "primary_failed", "cleanup_failed")
CASE_FIXED = {
    "primary_deadline_ms": 360_000, "outer_deadline_ms": 480_000,
    "cleanup_reserve_ms": 120_000, "stop_timeout_ms": 75_000,
    "client_reap_ms": 30_000, "startup_request_timeout_ms": 30_000,
    "request_timeout_ms": 75_000,
}
CASE_TRUE = (
    "exact_capabilities", "trust_off_rejected", "trust_off_client_reaped",
    "model_without_session_rejected", "dependencies_without_session_rejected",
    "local_frontend_gates", "async_begin", "read_while_starting", "ready",
    "ascii_control_home", "maven_nature", "custom_source", "compiler_17",
    "exact_dependency_reference", "dependency_insight", "frontend_binding",
    "pom_restart_required", "dependencies_pom_restart_required", "source_unchanged",
    "pom_expected", "repository_inputs_unchanged", "client_reaped",
    "synthetic_root_removed", "success",
)
CASE_PRESENT = (
    "jar_present_before", "jar_present_after", "pom_present_before", "pom_present_after",
    "hover", "completion", "deliberate_type_diagnostic", "dirty_change_acknowledged",
    "correction_diagnostics", "no_autosave",
)
CASE_FALSE = (
    "primary_failed", "cleanup_failed", "budget_refused", "elapsed_saturated",
    "startup_cleanup_verified",
    "rejected_diagnostic_source_java", "rejected_diagnostic_zero_range",
    "rejected_diagnostic_dependency_jar_absent", "rejected_diagnostic_dependency_pom_absent",
)
CASE_FLAGS = CASE_TRUE + CASE_PRESENT + CASE_FALSE + (
    "offline_pom_diagnostic", "project_missing_library_diagnostic",
)
EVENT_ENUMS = {
    "event_probe_outcome": ("not_attempted", "request_failed", "non_language_payload",
                            "response_received", "events_accepted", "events_rejected"),
    "event_error_code": ("none", "unsupported_operation", "run_disabled", "language_not_running",
                         "language_maven_session_required", "language_maven_unsupported",
                         "language_maven_restart_required", "language_maven_invalid_model",
                         "language_maven_invalid_dependencies", "language_maven_stale_snapshot",
                         "transport_failure", "other"),
    "event_rejection": ("none", "truncated", "missing_events", "event_bound", "unknown_event",
                        "closed_event", "lagged_event", "missing_event_type", "missing_diagnostic_uri",
                        "missing_diagnostics", "diagnostic_bound", "unexpected_pom_diagnostic",
                        "unexpected_source_diagnostic", "unexpected_project_diagnostic",
                        "foreign_document", "uri_encoding", "other"),
    "rejected_diagnostic_origin": ("none", "pom", "source", "owned_project_root",
                                   "owned_project_without_trailing_slash", "foreign", "missing"),
    "rejected_diagnostic_code_shape": ("none", "missing", "string_zero", "string_type_mismatch",
                                       "string_invalid_classpath", "other_string", "integer_zero",
                                       "integer_type_mismatch", "integer_invalid_classpath",
                                       "other_integer", "other"),
    "rejected_diagnostic_severity": ("none", "missing", "error", "warning", "information", "hint", "other"),
    "rejected_diagnostic_message_class": ("none", "missing", "offline_owned_dependency",
                                          "plain_missing_owned_dependency", "deliberate_int_to_string",
                                          "unresolved_cedar_import", "unresolved_arithmetic",
                                          "owned_missing_maven_library", "other"),
}
EVENT_SUCCESS = {key: "events_accepted" if key == "event_probe_outcome" else "none" for key in EVENT_ENUMS}
CASE_ENUMS = {
    "case": ("present", "missing"),
    "failure_stage": ("none", "setup", "trust", "startup", "model", "dependencies",
                      "semantics", "pom_change", "stop", "client_reap", "fixture_cleanup"),
    "model_status": ("none", "imported", "unresolved", "unavailable"),
    "dependency_observation": ("none", "present", "absent", "not_observed"),
    **EVENT_ENUMS,
}
CASE_COUNTS = {
    "model_queries": (1, 240), "generated_metadata_files": (0, 128),
    "lifecycle_metadata_files": (0, 6), "lifecycle_metadata_mask": (0, 63),
    "foreign_repository_files": (0, 0), "generated_data_files": (1, 4096),
    "generated_data_bytes": (1, 134_217_728), "generated_project_files": (0, 256),
    "generated_project_bytes": (0, 16_777_216),
}
CASE_REPRESENTATION_MAX = {
    "model_queries": 65535, "generated_metadata_files": 65535,
    "lifecycle_metadata_files": 255, "lifecycle_metadata_mask": 255,
    "foreign_repository_files": 65535, "generated_data_files": 0xffffffff,
    "generated_data_bytes": 0xffffffffffffffff, "generated_project_files": 65535,
    "generated_project_bytes": 0xffffffffffffffff,
}
TOP_KEYS = set(TOP_FIXED) | set(TOP_ENUMS) | set(TOP_TRUE) | set(TOP_FALSE) | {"elapsed_ms", "cases"}
CASE_KEYS = set(CASE_FIXED) | set(CASE_FLAGS) | set(CASE_ENUMS) | set(CASE_COUNTS) | {"elapsed_ms", "stop"}


def bounded_integer(value, low, high):
    require(type(value) is int and low <= value <= high, "Invalid bounded integer")
    return value


def decode_case(value):
    """Copy only strict, public, finite fields, even when a case failed."""
    require(type(value) is dict and set(value) == CASE_KEYS, "Unexpected Maven case schema")
    result = {}
    for key in CASE_FLAGS:
        require(type(value[key]) is bool, "Maven case flag is not boolean")
        result[key] = value[key]
    for key, expected in CASE_FIXED.items():
        result[key] = bounded_integer(value[key], expected, expected)
    for key, choices in CASE_ENUMS.items():
        require(type(value[key]) is str and value[key] in choices, "Maven case enum is not allowlisted")
        result[key] = value[key]
    for key, high in CASE_REPRESENTATION_MAX.items():
        # Preserve bounded native counters after a failed inventory check;
        # only the success predicate can establish the accepted limits.
        result[key] = bounded_integer(value[key], 0, high)
    result["elapsed_ms"] = bounded_integer(value["elapsed_ms"], 0, CASE_FIXED["outer_deadline_ms"])
    stop = value["stop"]
    if stop is None:
        result["stop"] = None
    else:
        linux_java.validate_stop(stop, successful=False)
        result["stop"] = {key: stop[key] for key in (
            "platform", "status", "reason", "cleanup_joined",
            "shutdown_response_received", "exit_frame_completed",
        )}
        result["stop"]["root_exit"] = dict(stop["root_exit"])
    return result


def decode_probe(data):
    require(type(data) is bytes and len(data) <= MAX_LOG_BYTES, "Probe output exceeds bound")
    objects = [strict_json(line) for line in data.decode("utf-8", errors="strict").splitlines()
               if line.startswith("{")]
    require(len(objects) == 1 and type(objects[0]) is dict, "Expected one typed Linux Maven receipt")
    return decode_value(objects[0])


def decode_value(value):
    require(type(value) is dict and set(value) == TOP_KEYS, "Unexpected Maven pair schema")
    result = {}
    for key, expected in TOP_FIXED.items():
        result[key] = bounded_integer(value[key], 0 if key == "cache_input_files" else expected, expected)
    for key, expected in TOP_ENUMS.items():
        require(type(value[key]) is str and value[key] == expected, "Maven pair identity changed")
        result[key] = value[key]
    for key in TOP_TRUE + TOP_FALSE:
        require(type(value[key]) is bool, "Maven pair flag is not boolean")
        result[key] = value[key]
    result["elapsed_ms"] = bounded_integer(value["elapsed_ms"], 0, TOP_FIXED["pair_deadline_ms"])
    require(type(value["cases"]) is list and len(value["cases"]) == 2, "Expected exactly two Maven cases")
    result["cases"] = [decode_case(case) for case in value["cases"]]
    require([case["case"] for case in result["cases"]] == ["present", "missing"],
            "Maven cases must be ordered present then missing")
    return result


def validate_probe(value):
    # Direct predicate callers get the same strict noncoercing types as JSON.
    value = decode_value(value)
    require(value["cache_input_files"] == 83, "Maven cache file count mismatched")
    for key in TOP_TRUE:
        require(value[key] is True, "Incomplete Maven pair witness")
    for key in TOP_FALSE:
        require(value[key] is False, "Maven pair failed")
    bounded_integer(value["elapsed_ms"], 0, TOP_FIXED["pair_deadline_ms"] - 1)
    for case in value["cases"]:
        present = case["case"] == "present"
        require(case["failure_stage"] == "none", "Maven case did not complete")
        for key, expected in EVENT_SUCCESS.items():
            require(case[key] == expected, "Maven event probe contradicts a completed case")
        for key in CASE_TRUE:
            require(case[key] is True, "Incomplete Maven case witness")
        for key in CASE_FALSE:
            require(case[key] is False, "Maven case failed or used startup fallback")
        for key in CASE_PRESENT:
            require(case[key] is present, "Maven case presence or semantics mismatched")
        require(case["offline_pom_diagnostic"] is (not present), "Offline POM diagnostic mismatched")
        require(case["project_missing_library_diagnostic"] is (not present),
                "Owned missing-project diagnostic mismatched")
        require(case["model_status"] == ("imported" if present else "unresolved"),
                "Maven model status mismatched")
        require(case["dependency_observation"] in (("present",) if present else ("absent", "not_observed")),
                "Maven dependency observation mismatched")
        for key, (low, high) in CASE_COUNTS.items():
            bounded_integer(case[key], low, high)
        require(case["lifecycle_metadata_mask"].bit_count() == case["lifecycle_metadata_files"]
                and case["lifecycle_metadata_files"] <= case["generated_metadata_files"],
                "Lifecycle marker mask and counts disagree")
        bounded_integer(case["elapsed_ms"], 0, CASE_FIXED["outer_deadline_ms"] - 1)
        linux_java.validate_stop(case["stop"])
    require(sum(case["elapsed_ms"] for case in value["cases"]) <= value["elapsed_ms"],
            "Case durations exceed pair duration")
    return value


def parse_probe(data):
    return validate_probe(decode_probe(data))


def failure_probe(path):
    if path is None:
        return "not_run", None
    try:
        data = read_regular(path, MAX_LOG_BYTES)
        if not any(line.startswith(b"{") for line in data.splitlines()):
            return "unavailable", None
        return "available", decode_probe(data)
    except (ValueError, OSError, UnicodeError, TypeError, RecursionError, OverflowError):
        return "malformed", None


def prepare_archive(existing_archive, distribution):
    """Reuse a sealed input, or acquire only the shared official pin, in 180s."""
    distribution = Path(distribution)
    archive = distribution.parent / "jdtls-1.61.0.tar.gz"
    if existing_archive is not None:
        data = read_regular(existing_archive, linux_java.MAX_ARCHIVE_BYTES)
        require(hashlib.sha256(data).hexdigest() == linux_java.ARCHIVE_SHA256,
                "Existing archive checksum mismatch")
        linux_java.disk_preflight(shutil.disk_usage(distribution.parent).free, len(data))
        with archive.open("xb") as stream:
            stream.write(data)
        # The private copy exists, so an explicit input never falls back to
        # acquisition. No caller input is modified.
    # Omission permits only linux_java's existing fixed official URL, checksum,
    # disk preflight, download cap and safe extraction. There is no URL option.
    linux_java.prepare(archive, distribution)


def verify_cache(cache):
    cache = Path(cache).absolute()
    for ancestor in reversed(cache.parents):
        maven_cache.ordinary(ancestor, True)
    entries = maven_cache.load_manifest()
    require(len(entries) == 83 and sum(entry["bytes"] for entry in entries) == 4_065_288,
            "Frozen Maven cache scope changed")
    maven_cache.verify_cache(cache, entries)


def validate_test_listing(data):
    require(type(data) is bytes and len(data) <= MAX_LIST_BYTES, "Test listing exceeds bound")
    lines = [line for line in data.decode("utf-8", errors="strict").splitlines() if line]
    require(lines == [TEST_NAME + ": test", "1 test, 0 benchmarks"],
            "Exact ignored Linux Maven test was not uniquely listed")


def run(root, scratch_root, java, agent, existing_archive, cache):
    require(sys.platform == "linux" and platform.machine() == "x86_64", "Native Linux x86_64 is required")
    require(cache is not None, "Existing Maven cache is required")
    root = Path(root).resolve(strict=True)
    scratch_root = Path(scratch_root).resolve(strict=True)
    java = Path(java).resolve(strict=True)
    agent = Path(agent).resolve(strict=True)
    existing_archive = Path(existing_archive).absolute() if existing_archive is not None else None
    cache = Path(cache).absolute()
    require(str(scratch_root).isascii() and str(java).isascii(),
            "ASCII scratch root and Java executable paths are required")
    require(java.name == "java" and java.is_file() and os.access(java, os.X_OK), "Existing Java executable is required")
    require(agent.name == "cedar-agent" and agent.is_file() and os.access(agent, os.X_OK), "Prebuilt normal cedar-agent is required")
    agent_digest = hashlib.sha256(read_regular(agent, linux_java.MAX_AGENT_BYTES)).digest()
    release = read_regular(java.parent.parent / "release", 65536).decode("utf-8", errors="strict")
    require(re.search(r'^JAVA_VERSION="21(?:[.\-+][^"\r\n]*)?"$', release, re.MULTILINE), "Existing JDK 21 is required")
    require(scratch_root.is_dir(), "Scratch root must exist")
    evidence_path = scratch_root / EVIDENCE_NAME
    require(not evidence_path.exists() and not evidence_path.is_symlink(),
            "Sanitized evidence destination must be fresh")
    scratch = Path(tempfile.mkdtemp(prefix="cedar-linux-maven-", dir=scratch_root))
    stage = "source"
    runtime_log = None
    preparation_log = scratch / "archive-preparation-private.log"
    try:
        environment = os.environ.copy()
        for key in LAUNCHER_ENVIRONMENT_KEYS:
            environment.pop(key, None)
        environment["JAVA_HOME"] = str(java.parent.parent)
        environment["TMPDIR"] = str(scratch)
        require(all(key not in environment for key in LAUNCHER_ENVIRONMENT_KEYS),
                "Java and Maven launcher environment was not cleared")
        commit_log = scratch / "source-commit-private.log"
        dirty_log = scratch / "source-dirty-private.log"
        bounded_process(["git", "rev-parse", "--verify", "HEAD"], root, environment, commit_log, 10)
        source_commit = read_regular(commit_log, 128).decode("ascii").strip()
        require(re.fullmatch(r"[0-9a-f]{40}", source_commit), "Exact source commit is required")
        bounded_process(["git", "status", "--porcelain", "--untracked-files=normal"],
                        root, environment, dirty_log, 10)
        checkout_dirty = bool(read_regular(dirty_log, MAX_LOG_BYTES))
        stage = "archive_preparation"
        distribution = scratch / "JDT distribution 雪"
        driver = str(Path(__file__).resolve())
        prepare_command = [sys.executable, driver, "--prepare-distribution", str(distribution)]
        if existing_archive is not None:
            prepare_command.extend(["--prepare-archive", str(existing_archive)])
        bounded_process(prepare_command, root, environment,
                        preparation_log, PREPARE_TIMEOUT)
        stage = "cache_verification"
        bounded_process([sys.executable, driver, "--verify-cache", str(cache)], root, environment,
                        scratch / "cache-verification-private.log", CACHE_VERIFY_TIMEOUT)
        stage = "jdk"
        bounded_process([str(java), "-version"], scratch, environment, scratch / "jdk-private.log", 15)
        environment.update(CEDAR_JAVA=str(java), CEDAR_JDTLS_HOME=str(distribution),
                           CEDAR_AGENT_BIN=str(agent), CEDAR_MAVEN_CACHE_INPUT=str(cache))
        stage = "compile"
        compile_log = scratch / "compile-private.log"
        bounded_process(linux_java.test_compile_command(), root, environment, compile_log, COMPILE_TIMEOUT)
        require(hashlib.sha256(read_regular(agent, linux_java.MAX_AGENT_BYTES)).digest() == agent_digest,
                "Compilation changed the prebuilt normal agent")
        executable = compiled_test(read_regular(compile_log, MAX_LOG_BYTES))
        require(executable.is_relative_to(root / "target") and executable.is_file()
                and os.access(executable, os.X_OK), "Compiler returned an unexpected test executable")
        executable, sibling = linux_java.prepare_sibling_harness(scratch, executable, agent, agent_digest)
        environment["CEDAR_AGENT_BIN"] = str(sibling)
        stage = "selection"
        selection_log = scratch / "selection-private.log"
        bounded_process([str(executable), "--list", "--ignored", "--exact", TEST_NAME],
                        scratch, environment, selection_log, SELECTION_TIMEOUT)
        validate_test_listing(read_regular(selection_log, MAX_LIST_BYTES))
        stage = "acceptance"
        runtime_log = scratch / "acceptance-private.log"
        bounded_process([str(executable), "--ignored", "--exact", TEST_NAME, "--nocapture", "--test-threads=1"],
                        scratch, environment, runtime_log, TEST_TIMEOUT)
        probe = parse_probe(read_regular(runtime_log, MAX_LOG_BYTES))
        require(hashlib.sha256(read_regular(agent, linux_java.MAX_AGENT_BYTES)).digest() == agent_digest,
                "Acceptance changed the prebuilt normal agent")
        require(hashlib.sha256(read_regular(sibling, linux_java.MAX_AGENT_BYTES)).digest() == agent_digest,
                "Acceptance changed the sibling normal agent")
        stage = "cleanup"
        shutil.rmtree(scratch)
        require(not scratch.exists(), "Private acceptance scratch cleanup failed")
    except Exception as error:
        # Never echo exception messages or raw subprocess output. The strict
        # decoder reconstructs only finite fields from a failed Rust receipt.
        classification, sanitized = failure_probe(runtime_log)
        failure = {"schema_version": 1, "kind": "linux_maven_acceptance", "status": "failed", "stage": stage,
                   "raw_logs_published": False, "private_scratch_retained": scratch.exists(),
                   "probe_record_status": classification}
        if sanitized is not None:
            failure["probe"] = sanitized
        disk_failed = isinstance(error, linux_java.DiskPreflightError)
        if stage == "archive_preparation" and preparation_log.exists():
            try:
                disk_failed |= linux_java.DISK_FAILURE_MARKER in read_regular(preparation_log, MAX_LOG_BYTES).splitlines()
            except (ValueError, OSError):
                pass
        if disk_failed:
            failure["category"] = "disk_preflight"
        linux_java.publish(evidence_path, failure)
        raise RuntimeError("Linux Maven acceptance failed; raw details remain private") from None
    result = {
        "schema_version": 1, "kind": "linux_maven_acceptance", "status": "success",
        "jdt_version": "1.61.0", "jdt_archive_sha256": linux_java.ARCHIVE_SHA256,
        "pinned_archive_verified": True, "existing_jdk21_verified": True,
        "maven_cache_manifest_sha256": maven_cache.MANIFEST_SHA256,
        "maven_cache_files": 83, "maven_cache_bytes": 4_065_288,
        "maven_cache_verified": True, "existing_cache_reused": True,
        "existing_archive_reused": existing_archive is not None,
        "archive_acquisition_performed": existing_archive is None,
        "normal_agent_normal_client": True,
        "connection_route": "bundled_linux_sibling", "copied_agent_hash_verified": True,
        "normal_agent_unchanged": True, "source_commit": source_commit,
        "checkout_dirty": checkout_dirty, "source_snapshot": "before_preparation",
        "agent_sha256": agent_digest.hex(), "agent_build_provenance": "caller_supplied_prebuilt",
        "agent_source_equivalence_verified": False,
        "launcher_environment_cleared": True, "launcher_environment_keys_checked": 11,
        "shutdown_evidence": "backend_owned_linux_stop", "archive_preparation_timeout_s": PREPARE_TIMEOUT,
        "cache_verification_timeout_s": CACHE_VERIFY_TIMEOUT, "compile_timeout_s": COMPILE_TIMEOUT,
        "test_watchdog_s": TEST_TIMEOUT, "exact_test_selection_verified": True,
        "selection_timeout_s": SELECTION_TIMEOUT, "scratch_removed": True,
        "raw_logs_published": False, "gui_exercised": False, "maven_goals_exercised": False,
        "dependency_source_navigation_exercised": False, "network_isolation_verified": False,
        "probe": probe,
    }
    if existing_archive is not None:
        result["preparation_network_requests"] = 0
    linux_java.publish(evidence_path, result)
    return result


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scratch-root", default=os.environ.get("RUNNER_TEMP"))
    parser.add_argument("--java", default=os.environ.get("CEDAR_JAVA") or
                        (str(Path(os.environ["JAVA_HOME"]) / "bin/java") if "JAVA_HOME" in os.environ else None))
    parser.add_argument("--agent")
    parser.add_argument("--archive", help="Prefer an existing pinned JDT archive; otherwise acquire the same official pin")
    parser.add_argument("--cache", help="Required existing frozen Maven cache")
    parser.add_argument("--prepare-archive", help=argparse.SUPPRESS)
    parser.add_argument("--prepare-distribution", help=argparse.SUPPRESS)
    parser.add_argument("--verify-cache", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.prepare_archive or args.prepare_distribution:
        require(args.prepare_distribution and not args.verify_cache,
                "Archive preparation destination is required")
        try:
            prepare_archive(args.prepare_archive, args.prepare_distribution)
        except linux_java.DiskPreflightError:
            print(linux_java.DISK_FAILURE_MARKER.decode("ascii"))
            raise
        return
    if args.verify_cache:
        verify_cache(args.verify_cache)
        return
    root = Path(__file__).resolve().parent.parent
    require(args.scratch_root and args.java and args.cache,
            "Explicit scratch root, existing JDK 21 and Maven cache are required")
    run(root, args.scratch_root, args.java, args.agent or root / "target/release/cedar-agent",
        args.archive, args.cache)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("Linux Maven acceptance did not complete.", file=sys.stderr)
        sys.exit(1)
