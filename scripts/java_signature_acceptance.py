#!/usr/bin/env python3
"""Independent, same-run packaged-agent Parameter info proof (Python 3.12).

This driver emits only a bounded typed receipt. Raw child output, paths, Java
source and inherited environment values never enter public diagnostics. Compile
includes dependency acquisition; no offline cache is assumed. The Rust proof,
not killing its harness, establishes Java/agent cleanup. Failed scratch is kept.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform as host_platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import tomllib

from copy_draft_acceptance import AcceptanceError, compiled_test, require, strict_json
import linux_java_acceptance as jdt
import package_linux_agent_bundle as linux_bundle
import package_windows_bundle as windows_bundle

TEST_NAME = "java_parameter_info_acceptance_tests::real_jdt_parameter_info_uses_normal_agent_route"
SOURCE_FIXTURE_SHA256 = "2a196b0dc072953dcf36761bb91532ccbc6c64539b2d1cdcdb4857ab33045211"
EVIDENCE_NAME = "JAVA_SIGNATURE_ACCEPTANCE.json"
PROVENANCE_NAME = "VERIFIED_SIGNATURE_AGENT.json"
COMPILE_TIMEOUT = 600
PREPARE_TIMEOUT = 180
SELECTION_TIMEOUT = 15
RUNTIME_TIMEOUT = 780
REAP_TIMEOUT = 20
FINALIZE_TIMEOUT = 30
DRIVER_TIMEOUT = 1680
MAX_COMPILE_BYTES = 4 * 1024 * 1024
MAX_RUNTIME_BYTES = 256 * 1024
MAX_LIST_BYTES = 16 * 1024
MAX_RECEIPT_BYTES = 32 * 1024
MAX_SOURCE_BYTES = 16 * 1024 * 1024
MAX_SOURCES = 20000
MAX_SOURCE_TOTAL = 150 * 1024 * 1024
MAX_HARNESS_BYTES = 512 * 1024 * 1024
MAX_AGENT_BYTES = 128 * 1024 * 1024
HASH = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
STAGES = ("package", "source_commit", "source_inventory", "source_before", "jdk", "prepare",
          "compile", "sibling", "selection", "runtime", "source_after", "receipt", "complete")
CHILD_BOUNDS = {"source_commit": (10, 128), "source_inventory": (10, 4 * 1024 * 1024),
                "jdk": (15, 65536), "prepare": (PREPARE_TIMEOUT, 65536),
                "compile": (COMPILE_TIMEOUT, MAX_COMPILE_BYTES),
                "selection": (SELECTION_TIMEOUT, MAX_LIST_BYTES),
                "runtime": (RUNTIME_TIMEOUT, MAX_RUNTIME_BYTES)}
CATEGORIES = {"validation_failed", "subprocess_launch_error", "subprocess_timeout",
              "subprocess_output_limit", "subprocess_nonzero", "subprocess_io_error",
              "driver_error", "finalization_timeout", "driver_timeout", "receipt_io_error",
              "receipt_publication_unavailable"}
PROBE_FLAGS = (
    "primary_failed", "cleanup_failed", "exact_capabilities", "provider_supported",
    "open_version_acknowledged", "first_argument_labels_exact", "first_argument_active_exact",
    "second_argument_labels_exact", "second_argument_active_exact", "outside_call_empty",
    "source_unchanged", "startup_terminal", "stop_verified", "client_reaped",
    "root_exit_observed", "fixture_removed", "fixture_retained", "all_owned_terminal",
    "active_deadline_met", "cleanup_deadline_met", "elapsed_saturated",
)
PROBE_FIXED = {"schema_version": 1, "active_budget_ms": 570000, "cleanup_budget_ms": 180000,
               "watchdog_ms": 780000, "startup_budget_ms": 75000, "startup_request_ms": 30000,
               "request_ms": 75000, "reap_ms": 30000}
PROBE_COUNTS = {"capability_count": 32, "hello_count": 1, "list_count": 1,
                "read_count": 1, "begin_count": 1, "open_count": 1, "close_count": 1,
                "stop_count": 1, "cancel_count": 1, "poll_count": 1024,
                "cleanup_poll_count": 1024, "signature_count": 3,
                "first_result_count": 32, "second_result_count": 32, "outside_result_count": 32,
                "active_elapsed_ms": 570000, "cleanup_elapsed_ms": 180000, "elapsed_ms": 780000}
PROBE_STAGES = ("none", "setup", "connect", "capabilities", "list", "read", "begin", "poll",
                "open", "first_argument", "second_argument", "outside_call", "close",
                "source_check", "cleanup", "receipt")


class ChildFailure(AcceptanceError):
    def __init__(self, category, *, returncode=None, eof=False, reaped=False,
                 output_bytes=0, truncated=False):
        self.category = category if category in CATEGORIES else "validation_failed"
        super().__init__(self.category)
        self.returncode = returncode
        self.eof, self.reaped = eof, reaped
        self.output_bytes, self.truncated = output_bytes, truncated


def bounded_process(command, cwd, environment, timeout, limit):
    """One owned direct child, bounded nonblocking pipe and one failure teardown.

    No retry, process-name kill, process-tree discovery or cleanup of an uncertain
    descendant. The first failure is preserved even when direct-child reap fails.
    """
    started = time.monotonic()
    data = bytearray()
    eof, reaped, truncated, failure = False, False, False, None
    try:
        process = subprocess.Popen(command, cwd=cwd, env=environment, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    except (OSError, ValueError):
        raise ChildFailure("subprocess_launch_error") from None
    try:
        require(process.stdout is not None, "subprocess_io_error")
        os.set_blocking(process.stdout.fileno(), False)
        while not eof or process.poll() is None:
            require(time.monotonic() - started < timeout, "subprocess_timeout")
            chunk = None
            if not eof:
                try:
                    chunk = os.read(process.stdout.fileno(), min(65536, limit - len(data) + 1))
                except BlockingIOError:
                    pass
                if chunk == b"":
                    eof = True
                elif chunk is not None:
                    available = limit - len(data)
                    data.extend(chunk[:available])
                    truncated = len(chunk) > available
                    require(not truncated, "subprocess_output_limit")
            if chunk is None:
                time.sleep(min(0.01, max(0, timeout - (time.monotonic() - started))))
        require(time.monotonic() - started < timeout, "subprocess_timeout")
        require(process.returncode == 0, "subprocess_nonzero")
        # poll observed and reaped our direct child; this says nothing about JDT.
        reaped = True
    except AcceptanceError as error:
        failure = str(error)
    except OSError:
        failure = "subprocess_io_error"
    finally:
        if failure is not None:
            try:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=REAP_TIMEOUT)
                reaped = True
            except (OSError, subprocess.TimeoutExpired):
                reaped = False
        if process.stdout is not None:
            try:
                process.stdout.close()
            except OSError:
                failure = failure or "subprocess_io_error"
    if failure is not None:
        raise ChildFailure(failure, returncode=process.returncode, eof=eof, reaped=reaped,
                           output_bytes=len(data), truncated=truncated) from None
    return bytes(data)


def regular(path, limit):
    """Reject symlinks and component redirects before a bounded identity read."""
    path = Path(path)
    require(path.is_absolute(), "validation_failed")
    current = Path(path.anchor)
    for part in path.parts[1:]:
        current /= part
        info = current.lstat()
        require(not stat.S_ISLNK(info.st_mode)
                and not getattr(info, "st_file_attributes", 0) & 0x400, "validation_failed")
    before = path.stat()
    require(stat.S_ISREG(before.st_mode) and 0 <= before.st_size <= limit, "validation_failed")
    with path.open("rb") as stream:
        opened = os.fstat(stream.fileno())
        require((opened.st_dev, opened.st_ino, opened.st_size) ==
                (before.st_dev, before.st_ino, before.st_size), "validation_failed")
        data = stream.read(limit + 1)
        after = os.fstat(stream.fileno())
    require(len(data) == before.st_size and len(data) <= limit
            and (after.st_size, after.st_mtime_ns) == (before.st_size, before.st_mtime_ns),
            "validation_failed")
    return data


def digest(path, limit):
    return hashlib.sha256(regular(path, limit)).hexdigest()


def native_platform():
    value = {"linux": "linux", "win32": "windows"}.get(sys.platform)
    require(value is not None and host_platform.machine().lower() in ("x86_64", "amd64"),
            "validation_failed")
    return value


def identity(commit, run_url):
    require(type(commit) is str and COMMIT.fullmatch(commit)
            and type(run_url) is str and windows_bundle.RUN_URL.fullmatch(run_url), "validation_failed")


def package_module(platform):
    require(platform in ("linux", "windows"), "validation_failed")
    return linux_bundle if platform == "linux" else windows_bundle


def package_sources(root, module, manifest, payload):
    """Invert SOURCE_FILES: archive guide names are not checkout file paths."""
    reverse = {destination: source for source, destination in module.SOURCE_FILES.items()}
    require(len(reverse) == len(module.SOURCE_FILES), "validation_failed")
    binaries = {"cedar-agent"} if module is linux_bundle else set(module.BINARIES)
    for name, content in payload.items():
        if name in binaries or name == module.MANIFEST:
            continue
        source = reverse.get(name, name)
        require(source in module.SOURCE_FILES or module.is_notice(source), "validation_failed")
        actual = regular(root / source, module.MAX_TEXT_BYTES)
        require(actual == content or actual.replace(b"\r\n", b"\n") == content, "validation_failed")
    version = tomllib.loads(regular(root / "Cargo.toml", 65536).decode("utf-8"))["workspace"]["package"]["version"]
    require(type(version) is str and version == manifest["version"], "validation_failed")


def verify_artifact(root, artifact, verified, commit, run_url, platform):
    identity(commit, run_url)
    module = package_module(platform)
    extension = ".tar.gz" if platform == "linux" else ".zip"
    candidates = [path for path in artifact.iterdir() if path.name.endswith(extension)]
    require(len(candidates) == 1, "validation_failed")
    archive = candidates[0]
    data = regular(archive, module.MAX_ARCHIVE_BYTES)
    manifest, payload = module.verify_bytes(data, expected_commit=commit, expected_ci=run_url)
    package_sources(root, module, manifest, payload)
    name = "cedar-agent.exe" if platform == "windows" else "cedar-agent"
    content = payload[name]
    agent_sha = hashlib.sha256(content).hexdigest()
    records = {entry["path"]: entry for entry in manifest["files"]}
    require(records[name]["sha256"] == agent_sha and 0 < len(content) <= MAX_AGENT_BYTES,
            "validation_failed")
    require(not verified.exists(), "validation_failed")
    verified.mkdir(mode=0o700)
    with (verified / name).open("xb") as output:
        output.write(content)
    if platform == "linux":
        (verified / name).chmod(0o700)
    require(digest(verified / name, MAX_AGENT_BYTES) == agent_sha, "validation_failed")
    result = {"schema_version": 1, "platform": platform, "source_commit": commit,
              "ci_run_url": run_url, "source_version": manifest["version"],
              "artifact": "cedar-linux-agent-development" if platform == "linux" else "cedar-windows-development",
              "archive_sha256": hashlib.sha256(data).hexdigest(), "agent_sha256": agent_sha,
              "package_verified": True, "packaged_source_matches_checkout": True}
    publish(verified / PROVENANCE_NAME, result)
    return result


def validate_provenance(data, commit, run_url, platform):
    value = strict_json(data)
    required = {"schema_version", "platform", "source_commit", "ci_run_url", "source_version",
                "artifact", "archive_sha256", "agent_sha256", "package_verified", "packaged_source_matches_checkout"}
    require(type(value) is dict and set(value) == required, "validation_failed")
    identity(commit, run_url)
    require(type(value["schema_version"]) is int and value["schema_version"] == 1
            and value["platform"] == platform and value["source_commit"] == commit
            and value["ci_run_url"] == run_url and value["package_verified"] is True
            and value["packaged_source_matches_checkout"] is True, "validation_failed")
    require(value["artifact"] == ("cedar-linux-agent-development" if platform == "linux" else "cedar-windows-development")
            and type(value["source_version"]) is str and windows_bundle.VERSION.fullmatch(value["source_version"]),
            "validation_failed")
    require(all(type(value[key]) is str and HASH.fullmatch(value[key]) for key in ("archive_sha256", "agent_sha256")),
            "validation_failed")
    return value


def source_inventory(data):
    require(type(data) is bytes and len(data) <= 4 * 1024 * 1024 and data.endswith(b"\0"), "validation_failed")
    result = {}
    for record in data[:-1].split(b"\0"):
        header, raw_name = record.split(b"\t", 1)
        mode, kind, object_id = header.decode("ascii").split(" ")
        name = raw_name.decode("utf-8")
        parts = PurePosixPath(name)
        require(mode in ("100644", "100755") and kind == "blob" and COMMIT.fullmatch(object_id)
                and name and len(name) <= 512 and not parts.is_absolute()
                and "\\" not in name and ":" not in name
                and all(part not in ("", ".", "..") for part in name.split("/"))
                and not any(ord(character) < 32 for character in name)
                and name not in result, "validation_failed")
        result[name] = object_id
    require(0 < len(result) <= MAX_SOURCES and "Cargo.toml" in result, "validation_failed")
    return result


class FinalizationBudget:
    """Hashing before/after, copy verification and final receipt share 30 s."""
    def __init__(self):
        self.used = 0.0

    def call(self, function, *args):
        started = time.monotonic()
        try:
            require(self.used < FINALIZE_TIMEOUT, "finalization_timeout")
            result = function(*args)
        finally:
            self.used += time.monotonic() - started
        require(self.used < FINALIZE_TIMEOUT, "finalization_timeout")
        return result


def snapshot(root, inventory):
    result, total = {}, 0
    started = time.monotonic()
    for name, object_id in inventory.items():
        require(time.monotonic() - started < FINALIZE_TIMEOUT, "finalization_timeout")
        content = regular(root / name, MAX_SOURCE_BYTES)
        total += len(content)
        require(total <= MAX_SOURCE_TOTAL, "validation_failed")
        canonical = content if windows_bundle.blob_hash(content) == object_id else content.replace(b"\r\n", b"\n")
        require(windows_bundle.blob_hash(canonical) == object_id, "validation_failed")
        result[name] = hashlib.sha256(content).hexdigest()
    return result


def validate_probe(data, platform, *, successful):
    require(type(data) is bytes and len(data) <= MAX_RECEIPT_BYTES, "validation_failed")
    value = strict_json(data)
    expected = set(PROBE_FLAGS) | set(PROBE_FIXED) | set(PROBE_COUNTS) | {
        "kind", "platform", "route", "status", "failure_stage", "source_fixture_sha256"}
    require(type(value) is dict and set(value) == expected, "validation_failed")
    require(value["kind"] == "java_parameter_info_normal_agent" and value["platform"] == platform
            and value["route"] == "normal_agent_normal_client" and value["status"] in ("passed", "failed")
            and value["failure_stage"] in PROBE_STAGES, "validation_failed")
    for key in PROBE_FLAGS:
        require(type(value[key]) is bool, "validation_failed")
    for key, number in PROBE_FIXED.items():
        require(type(value[key]) is int and value[key] == number, "validation_failed")
    for key, high in PROBE_COUNTS.items():
        require(type(value[key]) is int and 0 <= value[key] <= high, "validation_failed")
    require(type(value["source_fixture_sha256"]) is str
            and value["source_fixture_sha256"] == SOURCE_FIXTURE_SHA256, "validation_failed")
    require(value["elapsed_ms"] >= value["active_elapsed_ms"] + value["cleanup_elapsed_ms"], "validation_failed")
    if successful:
        require(value["status"] == "passed" and value["failure_stage"] == "none", "validation_failed")
        false = {"primary_failed", "cleanup_failed", "fixture_retained", "elapsed_saturated"}
        require(all(value[key] is (key not in false) for key in PROBE_FLAGS), "validation_failed")
        exact = {"capability_count": 32, "hello_count": 1, "list_count": 1, "read_count": 1,
                 "begin_count": 1, "open_count": 1, "close_count": 1, "stop_count": 1,
                 "cancel_count": 0, "cleanup_poll_count": 0, "signature_count": 3,
                 "first_result_count": 2, "second_result_count": 2, "outside_result_count": 0}
        require(all(value[key] == number for key, number in exact.items()) and 1 <= value["poll_count"] <= 1024,
                "validation_failed")
    return value


def validate_listing(data):
    require(type(data) is bytes and len(data) <= MAX_LIST_BYTES, "validation_failed")
    lines = [line for line in data.decode("utf-8").splitlines() if line]
    require(lines == [TEST_NAME + ": test", "1 test, 0 benchmarks"], "validation_failed")


def validate_execution(data):
    require(type(data) is bytes and len(data) <= MAX_RUNTIME_BYTES, "validation_failed")
    lines = data.decode("utf-8").splitlines()
    require([line for line in lines if line.startswith("running ")] == ["running 1 test"], "validation_failed")
    require([line for line in lines if line.startswith("test ") and not line.startswith("test result:")] ==
            ["test " + TEST_NAME + " ... ok"], "validation_failed")
    summaries = [line for line in lines if line.startswith("test result:")]
    require(len(summaries) == 1 and re.fullmatch(
        r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s",
        summaries[0]), "validation_failed")


def compile_command():
    return ["cargo", "test", "-p", "cedar-app", "--lib", "--locked", "--no-run", "--message-format=json"]


def copy_siblings(scratch, executable, agent, expected):
    destination = scratch / "Local bundle"
    destination.mkdir(mode=0o700)
    harness = destination / ("cedar-signature-proof.exe" if agent.suffix == ".exe" else "cedar-signature-proof")
    sibling = destination / agent.name
    for original, target, limit in ((executable, harness, MAX_HARNESS_BYTES), (agent, sibling, MAX_AGENT_BYTES)):
        content = regular(original, limit)
        require(bool(content), "validation_failed")
        with target.open("xb") as output:
            output.write(content)
        shutil.copymode(original, target)
        require(digest(target, limit) == hashlib.sha256(content).hexdigest(), "validation_failed")
    require(digest(sibling, MAX_AGENT_BYTES) == expected, "validation_failed")
    return harness, sibling


def publish(path, value):
    data = (json.dumps(value, sort_keys=True, ensure_ascii=True, allow_nan=False) + "\n").encode()
    require(len(data) <= MAX_RECEIPT_BYTES, "validation_failed")
    with path.open("xb") as output:
        output.write(data)


def publish_final(path, result, budget):
    """Account for serialization, write and atomic publication, including failure.

    A late post-publication check gets exactly one fixed incomplete replacement.
    That replacement is also charged to the shared clock, even when the original
    budget was exhausted. If it fails, withhold only this driver's public receipt;
    no private fixture, process or dependency cleanup is attempted.
    """
    result = dict(result, receipt_publication_verified=True,
                  receipt_failure_category="none")
    pending = path.with_name(path.name + ".pending")
    started = time.monotonic()
    publication_failure = None
    try:
        require(budget.used < FINALIZE_TIMEOUT, "finalization_timeout")
        publish(pending, result)
        require(budget.used + time.monotonic() - started < FINALIZE_TIMEOUT,
                "finalization_timeout")
        os.replace(pending, path)
        require(budget.used + time.monotonic() - started < FINALIZE_TIMEOUT,
                "finalization_timeout")
    except Exception as error:
        publication_failure = ("finalization_timeout" if isinstance(error, AcceptanceError)
                               and str(error) == "finalization_timeout" else "receipt_io_error")
    finally:
        budget.used += time.monotonic() - started
    if publication_failure is None and budget.used >= FINALIZE_TIMEOUT:
        publication_failure = "finalization_timeout"
    if publication_failure is None:
        return result
    # Preserve the primary failed stage/category, if there already was one.
    if result["status"] == "passed":
        result.update(status="failed", stage="receipt", category=publication_failure)
    result.update(partial_evidence=True, receipt_publication_verified=False,
                  receipt_failure_category=publication_failure)
    fallback = path.with_name(path.name + ".incomplete")
    started = time.monotonic()
    try:
        publish(fallback, result)
        os.replace(fallback, path)
    except Exception:
        # Never leave an earlier passed artifact after a failed budget check.
        # This is solely the newly generated public receipt, not scratch cleanup.
        if path.exists():
            path.unlink()
        raise AcceptanceError("receipt_publication_unavailable") from None
    finally:
        budget.used += time.monotonic() - started
    return result


def run(root, artifact, verified, scratch_root, commit, run_url, *, runner=bounded_process):
    platform = native_platform()
    evidence = scratch_root / EVIDENCE_NAME
    require(not evidence.exists(), "validation_failed")
    stage, probe_path, scratch, failure = "package", None, None, None
    provenance, probe, before = None, None, None
    budget = FinalizationBudget()
    started = time.monotonic()
    try:
        provenance = validate_provenance(regular(verified / PROVENANCE_NAME, MAX_RECEIPT_BYTES), commit, run_url, platform)
        agent = verified / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
        require(budget.call(digest, agent, MAX_AGENT_BYTES) == provenance["agent_sha256"], "validation_failed")
        scratch = Path(tempfile.mkdtemp(prefix="cedar-signature-", dir=scratch_root))
        environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", CARGO_TARGET_DIR=str(root / "target"))
        for key in (*jdt.JAVA_ENVIRONMENT_KEYS, "JDK_JAVAC_OPTIONS", "CLASSPATH", "CEDAR_AGENT_BIN"):
            environment.pop(key, None)
        java_home = Path(environment.get("JAVA_HOME_21_X64", ""))
        require(java_home.is_absolute(), "validation_failed")
        java = java_home / "bin" / ("java.exe" if platform == "windows" else "java")
        release = regular(java_home / "release", 65536).decode("utf-8")
        require(re.search(r'^JAVA_VERSION="21(?:[.\-+][^"\r\n]*)?"$', release, re.MULTILINE), "validation_failed")
        environment.update(JAVA_HOME=str(java_home), TMPDIR=str(scratch), TMP=str(scratch), TEMP=str(scratch))
        stage = "source_commit"
        actual = runner(["git", "rev-parse", "--verify", "HEAD"], root, environment, 10, 128).decode("ascii").strip()
        require(actual == commit, "validation_failed")
        stage = "source_inventory"
        inventory = source_inventory(runner(["git", "ls-tree", "-r", "-z", "HEAD"], root, environment, 10, 4 * 1024 * 1024))
        stage = "source_before"
        before = budget.call(snapshot, root, inventory)
        stage = "jdk"
        runner([str(java), "-version"], scratch, environment, 15, 65536)
        stage = "prepare"
        distribution = scratch / "JDT distribution"
        runner([sys.executable, "-B", str(root / "scripts/java_signature_acceptance.py"), "--prepare",
                str(scratch / "jdtls-1.61.0.tar.gz"), str(distribution)], root, environment, PREPARE_TIMEOUT, 65536)
        stage = "compile"
        output = runner(compile_command(), root, environment, COMPILE_TIMEOUT, MAX_COMPILE_BYTES)
        executable = compiled_test(output, root, platform)
        stage = "sibling"
        executable, sibling = budget.call(copy_siblings, scratch, executable, agent, provenance["agent_sha256"])
        probe_path = scratch / "probe.json"
        environment.update(CEDAR_REAL_JDT_PARAMETER_INFO="1", CEDAR_REAL_JDT_PARAMETER_INFO_RECEIPT=str(probe_path),
                           CEDAR_JDT_LS_ROOT=str(distribution), CEDAR_JAVA_COMMAND=str(java))
        stage = "selection"
        validate_listing(runner([str(executable), TEST_NAME, "--ignored", "--exact", "--list"],
                                scratch, environment, SELECTION_TIMEOUT, MAX_LIST_BYTES))
        stage = "runtime"
        validate_execution(runner([str(executable), TEST_NAME, "--ignored", "--exact", "--test-threads=1", "--nocapture"],
                                  scratch, environment, RUNTIME_TIMEOUT, MAX_RUNTIME_BYTES))
        probe = validate_probe(regular(probe_path, MAX_RECEIPT_BYTES), platform, successful=True)
        stage = "source_after"
        require(budget.call(snapshot, root, inventory) == before, "validation_failed")
        for path in (agent, sibling):
            require(budget.call(digest, path, MAX_AGENT_BYTES) == provenance["agent_sha256"], "validation_failed")
        module = package_module(platform)
        archives = [path for path in artifact.iterdir() if path.name.endswith(".tar.gz" if platform == "linux" else ".zip")]
        require(len(archives) == 1 and budget.call(digest, archives[0], module.MAX_ARCHIVE_BYTES) == provenance["archive_sha256"],
                "validation_failed")
        require(time.monotonic() - started < DRIVER_TIMEOUT - 20, "driver_timeout")
        stage = "complete"
    except Exception as error:
        failure = error
    # No scratch deletion or further process activity after an uncertain result.
    probe_state = "valid" if probe is not None else "absent"
    if probe is None and probe_path is not None:
        try:
            probe = validate_probe(regular(probe_path, MAX_RECEIPT_BYTES), platform, successful=False)
            probe_state = "valid"
        except FileNotFoundError:
            pass
        except Exception:
            probe_state = "invalid"
    category = (failure.category if isinstance(failure, ChildFailure) else
                str(failure) if isinstance(failure, AcceptanceError) and str(failure) in CATEGORIES else "driver_error") if failure else "none"
    result = {"schema_version": 1, "kind": "java_signature_acceptance", "status": "failed" if failure else "passed",
              "stage": stage, "category": category, "platform": platform, "source_commit": commit, "ci_run_url": run_url,
              "jdt_version": "1.61.0", "jdt_archive_sha256": jdt.ARCHIVE_SHA256,
              "raw_output_published": False, "native_gui_proved": False, "cross_run_artifact_allowed": False,
              "source_before_after_verified": failure is None, "packaged_agent_unchanged": failure is None,
              "partial_evidence": failure is not None, "private_scratch_retained": scratch is not None,
              "probe_record_status": probe_state, "probe": probe, "provenance": provenance,
              "child_failure": None}
    if isinstance(failure, ChildFailure):
        code = failure.returncode
        timeout, limit = CHILD_BOUNDS[stage]
        result["child_failure"] = {"category": category, "returncode": code if type(code) is int and -(2**31) <= code <= 2**32 - 1 else None,
                                   "eof": failure.eof, "direct_child_reaped": failure.reaped,
                                   "timeout_s": timeout, "output_limit_bytes": limit,
                                   "output_bytes": min(limit, max(0, failure.output_bytes)), "output_truncated": failure.truncated,
                                   "descendant_cleanup_inferred": False}
    return publish_final(evidence, result, budget)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare", nargs=2, metavar=("ARCHIVE", "DISTRIBUTION"))
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--artifact-dir", type=Path)
    parser.add_argument("--verified-dir", type=Path)
    parser.add_argument("--scratch-root", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.prepare:
            archive, distribution = map(Path, args.prepare)
            jdt.prepare(archive, distribution)
            config = "config_win" if native_platform() == "windows" else "config_linux"
            require((distribution / config).is_dir(), "validation_failed")
            return 0
        root = Path(__file__).resolve().parent.parent
        commit = os.environ.get("GITHUB_SHA", "")
        run_url = os.environ.get("GITHUB_SERVER_URL", "") + "/" + os.environ.get("GITHUB_REPOSITORY", "") + "/actions/runs/" + os.environ.get("GITHUB_RUN_ID", "")
        identity(commit, run_url)
        require(args.artifact_dir is not None and args.verified_dir is not None, "validation_failed")
        artifact, verified = args.artifact_dir.absolute(), args.verified_dir.absolute()
        if args.verify:
            verify_artifact(root, artifact, verified, commit, run_url, native_platform())
            print("Same-run package and mapped checkout sources verified.")
            return 0
        require(args.scratch_root is not None, "validation_failed")
        result = run(root, artifact, verified, args.scratch_root.absolute(), commit, run_url)
        print("Parameter info acceptance: " + result["status"] + "; stage=" + result["stage"] + "; category=" + result["category"])
        return 0 if result["status"] == "passed" else 1
    except Exception:
        # Also preserve a bounded partial record when package verification (or
        # initial argument/provenance validation) fails before run() starts.
        if args.scratch_root is not None:
            try:
                path = args.scratch_root.absolute() / EVIDENCE_NAME
                if not path.exists():
                    publish(path, {"schema_version": 1, "kind": "java_signature_acceptance",
                                   "status": "failed", "stage": "package", "category": "validation_failed",
                                   "partial_evidence": True, "raw_output_published": False,
                                   "native_gui_proved": False, "probe_record_status": "absent"})
            except Exception:
                pass
        print("Parameter info acceptance failed: validation_failed; raw diagnostics withheld.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
