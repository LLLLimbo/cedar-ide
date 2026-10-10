#!/usr/bin/env python3
"""Run the exact Linux shipping-agent typed Java acceptance, with private logs.

Preparation is bounded separately from the 720-second test process watchdog.
JDT provenance is the existing pinned official archive and SHA-256, not a
signature claim. An installed JDK 21 and prebuilt normal cedar-agent are required.
No Maven, GUI, SSH, deployment, or network-isolation acceptance is implied.
Run as a standalone CLI with exclusive child-reaper ownership: competing
SIGCHLD reapers and SIGCHLD auto-reap are unsupported.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request

ARCHIVE_URL = "https://download.eclipse.org/jdtls/milestones/1.61.0/jdt-language-server-1.61.0-202609031315.tar.gz"
ARCHIVE_SHA256 = "338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64"
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024
MAX_EXTRACTED_BYTES = 1024 * 1024 * 1024
DISK_RESERVE_BYTES = 1024 * 1024 * 1024
DISK_FAILURE_MARKER = b'{"category":"disk_preflight"}'
MAX_MEMBER_BYTES = 128 * 1024 * 1024
MAX_MEMBERS = 10_000
MAX_LOG_BYTES = 16 * 1024 * 1024
MAX_AGENT_BYTES = 256 * 1024 * 1024
JAVA_ENVIRONMENT_KEYS = ("CLIENT_PORT", "CLIENT_HOST", "socket.stream.debug",
                         "JDK_JAVA_OPTIONS", "JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS")
EVIDENCE_NAME = "LINUX_JAVA_ACCEPTANCE.json"
PREPARE_TIMEOUT = 180
COMPILE_TIMEOUT = 600
TEST_TIMEOUT = 720
SELECTION_TIMEOUT = 15
MAX_LIST_BYTES = 16 * 1024
TEST_NAME = "language_ui::real_java_tests::acceptance::linux::real_linux_normal_agent_java_editor_acceptance"
BOOLS = (
    "exact_capabilities", "maven_unadvertised", "trust_off_rejected",
    "trust_off_client_reaped", "async_begin", "read_while_starting", "ready",
    "selected_external_data", "semantic_diagnostics", "hover", "exact_definition",
    "real_completion", "deferred_import_resolve", "editor_apply_undo_redo",
    "versions_2_3_4_synced", "correction_acknowledged", "workflow_success",
    "main_deadline_met", "restart_deadline_met",
    "refresh_supported", "refresh_acknowledged", "refresh_witness", "refresh_unversioned",
    "organize_imports", "implementations", "source_files_unchanged",
    "close_acknowledged", "same_agent_restart", "restart_ready",
    "restart_read_while_starting", "client_reaped", "synthetic_root_removed",
)
FALSE_BOOLS = (
    "primary_failed", "cleanup_failed", "restart_failed", "startup_cleanup_verified", "elapsed_saturated",
)
FIXED = {
    "schema_version": 1, "capability_count": 31,
    "organize_editor_stages": 5, "implementation_type_count": 2,
    "implementation_method_count": 1, "implementation_negative_count": 0,
    "primary_deadline_ms": 360_000, "outer_deadline_ms": 480_000,
    "cleanup_reserve_ms": 120_000, "startup_deadline_ms": 75_000,
    "startup_request_timeout_ms": 30_000, "request_timeout_ms": 75_000,
    "spontaneous_dispatch_window_ms": 60_000, "recovery_admission_ms": 240_000,
    "restart_deadline_ms": 180_000, "client_reap_ms": 30_000,
}
ENUMS = {
    "kind": "linux_java_production", "route": "normal_agent_normal_client",
    "status": "success", "failure_stage": "none",
}
CORRECTION = {"spontaneous_result", "spontaneous_success", "recovery_attempts",
              "recovery_result", "recovery_acknowledged", "recovery_witness",
              "recovery_unversioned", "recovery_budget_sufficient"}
COUNTS = {
    "organize_main_edits": (1, 65535), "organize_ambiguity_edits": (1, 65535),
    "elapsed_ms": (0, 719999), "main_elapsed_ms": (0, 479999),
    "restart_elapsed_ms": (0, 179999),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate receipt key")
        result[key] = value
    return result


def strict_json(text):
    def nonfinite(_):
        raise ValueError("Nonfinite receipt number")
    return json.loads(text, object_pairs_hook=unique_object, parse_constant=nonfinite)


def validate_stop(value, *, successful=True):
    require(type(value) is dict and set(value) == {
        "platform", "status", "reason", "root_exit", "cleanup_joined",
        "shutdown_response_received", "exit_frame_completed",
    }, "Unexpected Stop receipt schema")
    require(value["platform"] == "linux" and type(value["platform"]) is str,
            "Stop platform is not Linux")
    require(value["status"] in (("graceful", "forced") if successful else ("graceful", "forced", "error")), "Invalid Stop status")
    require(value["reason"] in ("root_exited", "grace_expired", "aborted",
                                "transport_failure", "worker_panicked"), "Invalid Stop reason")
    require(value["cleanup_joined"] is True, "Stop did not verify owned cleanup")
    for key in ("shutdown_response_received", "exit_frame_completed"):
        require(type(value[key]) is bool, "Stop protocol flag is not boolean")
    exit_value = value["root_exit"]
    require(type(exit_value) is dict, "Stop lacks a tagged root exit")
    if exit_value.get("kind") == "code":
        require(set(exit_value) == {"kind", "code"} and type(exit_value["code"]) is int
                and 0 <= exit_value["code"] <= 255, "Invalid Linux exit code")
    elif exit_value.get("kind") == "signal":
        require(set(exit_value) == {"kind", "signal"} and type(exit_value["signal"]) is int
                and 1 <= exit_value["signal"] <= 64, "Invalid Linux exit signal")
    else:
        raise ValueError("Root exit is unobserved or untagged")
    if value["status"] == "graceful":
        require(value["reason"] == "root_exited" and exit_value == {"kind": "code", "code": 0}
                and value["shutdown_response_received"] and value["exit_frame_completed"],
                "Graceful Stop lacks natural code-zero protocol completion")
    return value


def validate_probe(receipt):
    require(type(receipt) is dict, "Probe receipt must be an object")
    expected = set(BOOLS) | set(FALSE_BOOLS) | set(FIXED) | set(ENUMS) | set(COUNTS) | CORRECTION | {"initial_stop", "restart_stop"}
    require(set(receipt) == expected, "Unexpected probe receipt schema")
    for key in BOOLS:
        require(receipt[key] is True, "Incomplete probe witness")
    for key in FALSE_BOOLS:
        require(receipt[key] is False, "Probe reported failed or recovered acceptance")
    for key, value in FIXED.items():
        require(type(receipt[key]) is int and receipt[key] == value, "Invalid fixed bound or count")
    for key, value in ENUMS.items():
        require(type(receipt[key]) is str and receipt[key] == value, "Invalid acceptance result")
    for key, (low, high) in COUNTS.items():
        require(type(receipt[key]) is int and low <= receipt[key] <= high, "Invalid bounded count")
    require(receipt["elapsed_ms"] >= receipt["main_elapsed_ms"] + receipt["restart_elapsed_ms"],
            "Phase durations exceed total duration")
    validate_correction(receipt)
    validate_stop(receipt["initial_stop"])
    validate_stop(receipt["restart_stop"])
    return receipt


def validate_correction(receipt):
    require(type(receipt["recovery_attempts"]) is int, "Recovery attempt count is not an integer")
    flags = ("spontaneous_success", "recovery_acknowledged", "recovery_witness",
             "recovery_unversioned", "recovery_budget_sufficient")
    require(all(type(receipt[key]) is bool for key in flags), "Correction flag is not boolean")
    if receipt["spontaneous_result"] == "matched":
        require(receipt["spontaneous_success"] and receipt["recovery_attempts"] == 0
                and receipt["recovery_result"] == "not_attempted"
                and all(not receipt[key] for key in flags[1:]), "Inconsistent spontaneous correction")
    elif receipt["spontaneous_result"] == "timeout":
        require(not receipt["spontaneous_success"] and receipt["recovery_attempts"] == 1
                and receipt["recovery_result"] == "matched" and receipt["recovery_acknowledged"]
                and receipt["recovery_witness"] and receipt["recovery_budget_sufficient"],
                "Recovery did not establish one exact admitted workflow witness")
    else:
        raise ValueError("Only a spontaneous timeout permits workflow recovery")


DIAGNOSTIC_RESULTS = ("matched", "timeout", "request_error", "malformed_events", "truncated", "lagged", "closed")
RECOVERY_RESULTS = ("not_attempted", "not_eligible", "insufficient_budget", "request_error",
                    "acknowledgement_mismatch", "timeout", "malformed_events", "truncated", "lagged", "closed", "matched")
FAILURE_STAGES = ("none", "setup", "initialize", "open", "diagnostics", "hover", "definition", "completion",
                  "resolve", "apply", "undo", "redo", "sync", "correction", "close", "stop", "root_exit", "agent_exit", "fixture_cleanup")


def decode_probe(data):
    """Reconstruct only finite public fields, including an incomplete receipt."""
    require(type(data) is bytes and len(data) <= MAX_LOG_BYTES, "Probe output exceeds bound")
    objects = [strict_json(line) for line in data.decode("utf-8", errors="strict").splitlines()
               if line.startswith("{")]
    require(len(objects) == 1 and type(objects[0]) is dict, "Expected one typed Linux receipt")
    receipt = objects[0]
    expected = set(BOOLS) | set(FALSE_BOOLS) | set(FIXED) | set(ENUMS) | set(COUNTS) | CORRECTION | {"initial_stop", "restart_stop"}
    require(set(receipt) == expected, "Unexpected probe receipt schema")
    result = {}
    flags = set(BOOLS) | set(FALSE_BOOLS) | {
        "spontaneous_success", "recovery_acknowledged", "recovery_witness", "recovery_unversioned", "recovery_budget_sufficient"}
    for key in flags:
        require(type(receipt[key]) is bool, "Probe flag is not boolean")
        result[key] = receipt[key]
    variable_fixed = {"capability_count": 31, "organize_editor_stages": 5,
                      "implementation_type_count": 128, "implementation_method_count": 128,
                      "implementation_negative_count": 128}
    for key, expected_value in FIXED.items():
        require(type(receipt[key]) is int, "Probe fixed count is not an integer")
        if key in variable_fixed:
            require(0 <= receipt[key] <= variable_fixed[key], "Probe count exceeds bound")
        else:
            require(receipt[key] == expected_value, "Probe identity or bound changed")
        result[key] = receipt[key]
    for key in COUNTS:
        high = 65535 if key.startswith("organize_") else 0xffffffff
        require(type(receipt[key]) is int and 0 <= receipt[key] <= high, "Probe count exceeds representation bound")
        result[key] = receipt[key]
    require(type(receipt["recovery_attempts"]) is int and 0 <= receipt["recovery_attempts"] <= 1,
            "Recovery attempt count exceeds bound")
    result["recovery_attempts"] = receipt["recovery_attempts"]
    enums = {"kind": (ENUMS["kind"],), "route": (ENUMS["route"],), "status": ("success", "failed"),
             "failure_stage": FAILURE_STAGES, "spontaneous_result": DIAGNOSTIC_RESULTS, "recovery_result": RECOVERY_RESULTS}
    for key, choices in enums.items():
        require(type(receipt[key]) is str and receipt[key] in choices, "Probe enum is not allowlisted")
        result[key] = receipt[key]
    for key in ("initial_stop", "restart_stop"):
        value = receipt[key]
        if value is None:
            result[key] = None
        else:
            validate_stop(value, successful=False)
            result[key] = {field: value[field] for field in (
                "platform", "status", "reason", "cleanup_joined", "shutdown_response_received", "exit_frame_completed")}
            result[key]["root_exit"] = dict(value["root_exit"])
    return result


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
    except (ValueError, OSError, UnicodeError, TypeError):
        return "malformed", None


def read_regular(path, limit):
    path = Path(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_size <= limit, "Expected bounded regular file")
    with path.open("rb") as stream:
        require(os.fstat(stream.fileno()).st_ino == before.st_ino
                and os.fstat(stream.fileno()).st_dev == before.st_dev,
                "File identity changed during open")
        data = stream.read(limit + 1)
    require(len(data) <= limit and len(data) == before.st_size, "File changed or exceeded bound")
    return data


def archive_member(member):
    name = member.name
    require(type(name) is str and name and len(name.encode("utf-8")) <= 4096
            and "\\" not in name and "\x00" not in name, "Unsafe archive member name")
    while name.startswith("./"):
        name = name[2:]
    if name in ("", ".") and member.isdir():
        require(member.size == 0, "Archive directory declares file data")
        return None
    path = PurePosixPath(name)
    require(not path.is_absolute() and all(part not in ("", ".", "..") for part in name.rstrip("/").split("/"))
            and len(path.parts) <= 32, "Archive member escapes destination")
    require(member.isdir() or member.isfile(), "Archive links and special files are forbidden")
    require(not member.issparse() and type(member.size) is int and 0 <= member.size <= MAX_MEMBER_BYTES,
            "Archive member size or sparse type exceeds bound")
    require(not member.isdir() or member.size == 0, "Archive directory declares file data")
    return path


class DiskPreflightError(ValueError):
    """Fixed public category; never includes a filesystem path."""


def disk_preflight(free_bytes, content_bytes):
    require(type(free_bytes) is int and 0 <= free_bytes <= 0xffffffffffffffff
            and type(content_bytes) is int and 0 <= content_bytes <= MAX_EXTRACTED_BYTES,
            "Invalid disk preflight byte count")
    if free_bytes < content_bytes + DISK_RESERVE_BYTES:
        raise DiskPreflightError("disk_preflight")


def scan_members(members):
    """Exact regular-file expansion under the same extraction safety bounds."""
    seen = set()
    expanded = 0
    for count, member in enumerate(members, start=1):
        require(count <= MAX_MEMBERS, "Archive member count exceeds bound")
        relative = archive_member(member)
        if relative is None:
            continue
        require(str(relative) not in seen, "Duplicate archive member")
        seen.add(str(relative))
        if member.isfile():
            expanded += member.size
            require(expanded <= MAX_EXTRACTED_BYTES, "Archive expansion exceeds bound")
    return expanded


def prepare(archive, distribution):
    """Invoked in one separately bounded subprocess; no archive code executes."""
    archive = Path(archive)
    distribution = Path(distribution)
    if not archive.exists():
        disk_preflight(shutil.disk_usage(archive.parent).free, MAX_ARCHIVE_BYTES)
        digest = hashlib.sha256()
        total = 0
        request = urllib.request.Request(ARCHIVE_URL, headers={"User-Agent": "cedar-linux-java-acceptance/1"})
        with urllib.request.urlopen(request, timeout=30) as response, archive.open("xb") as stream:
            require(response.geturl().startswith("https://"), "Archive redirect lost HTTPS")
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                require(total <= MAX_ARCHIVE_BYTES, "Archive download exceeds bound")
                digest.update(chunk)
                stream.write(chunk)
        require(digest.hexdigest() == ARCHIVE_SHA256, "Pinned JDT archive checksum mismatch")
    data = read_regular(archive, MAX_ARCHIVE_BYTES)
    require(hashlib.sha256(data).hexdigest() == ARCHIVE_SHA256, "Pinned JDT archive checksum mismatch")
    require(not distribution.exists(), "Distribution destination must be fresh")
    # The archive is already present. Scan every member before creating the
    # distribution, then reserve exact regular-file expansion plus 1 GiB for
    # data/compile/logs. This is a capacity check, not a bound on trusted JDT writes.
    with tarfile.open(archive, mode="r:gz") as source:
        expanded = scan_members(source)
    disk_preflight(shutil.disk_usage(distribution.parent).free, expanded)
    distribution.mkdir(mode=0o700)
    seen = set()
    total = 0
    with tarfile.open(archive, mode="r:gz") as source:
        for count, member in enumerate(source, start=1):
            require(count <= MAX_MEMBERS, "Archive member count exceeds bound")
            relative = archive_member(member)
            if relative is None:
                continue
            require(str(relative) not in seen, "Duplicate archive member")
            seen.add(str(relative))
            total += member.size
            require(total <= MAX_EXTRACTED_BYTES, "Archive expansion exceeds bound")
            destination = distribution.joinpath(*relative.parts)
            destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            if member.isdir():
                destination.mkdir(mode=0o700, exist_ok=True)
            else:
                extracted = source.extractfile(member)
                require(extracted is not None, "Archive file body missing")
                with extracted, destination.open("xb") as output:
                    remaining = member.size
                    while remaining:
                        chunk = extracted.read(min(remaining, 1024 * 1024))
                        require(chunk, "Archive file body truncated")
                        output.write(chunk)
                        remaining -= len(chunk)
                    require(not extracted.read(1), "Archive file exceeded declared size")
    require(total == expanded, "Archive expansion changed after preflight")
    launchers = list((distribution / "plugins").glob("org.eclipse.equinox.launcher_*.jar"))
    require((distribution / "config_linux").is_dir() and len(launchers) == 1
            and launchers[0].is_file(), "Pinned distribution lacks exact Linux recipe inputs")


def bounded_process(command, cwd, env, log, timeout):
    """One finite selector loop; the private sink never exceeds MAX_LOG_BYTES.

    Emergency termination never establishes Java or agent cleanup success.
    No reader thread can survive this supervisor, and output after root exit
    must still reach EOF within the original process deadline.
    """
    started = time.monotonic()
    total = 0
    with Path(log).open("xb") as stream:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
        owns_child = True
        try:
            require(process.stdout is not None, "Private output pipe missing")
            os.set_blocking(process.stdout.fileno(), False)
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                open_pipe = True
                root_exit = None
                while open_pipe or root_exit is None:
                    # WNOWAIT retains our original child identity until all
                    # output is drained; an exited root cannot free/reuse this
                    # process-group id before failure cleanup signals it.
                    try:
                        observed = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                    except ChildProcessError:
                        # Suppress our explicit signal/wait only. Popen may
                        # later perform its own destructor polling; this CLI
                        # does not support competing reapers or auto-reap.
                        owns_child = False
                        raise ValueError("Private child wait ownership was lost") from None
                    if observed is not None:
                        root_exit = observed
                    remaining = timeout - (time.monotonic() - started)
                    require(remaining > 0, "Private subprocess exceeded wall deadline")
                    for key, _ in selector.select(min(remaining, 0.1)):
                        # At most one byte beyond the cap is observed, never
                        # written. Pipe backpressure bounds the producer too.
                        chunk = os.read(key.fd, min(65536, MAX_LOG_BYTES - total + 1))
                        if not chunk:
                            selector.unregister(process.stdout)
                            open_pipe = False
                            continue
                        require(len(chunk) <= MAX_LOG_BYTES - total, "Private subprocess output exceeded bound")
                        stream.write(chunk)
                        total += len(chunk)
                require(root_exit.si_code == os.CLD_EXITED and root_exit.si_status == 0,
                        "Private subprocess failed")
                require(time.monotonic() - started < timeout, "Private subprocess returned after its deadline")
                process.wait(timeout=10)
                owns_child = False
        finally:
            if owns_child:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=10)
            if process.stdout is not None:
                process.stdout.close()


def validate_test_listing(data):
    require(type(data) is bytes and len(data) <= MAX_LIST_BYTES, "Test listing exceeds bound")
    lines = [line for line in data.decode("utf-8", errors="strict").splitlines() if line]
    require(lines == [TEST_NAME + ": test", "1 test, 0 benchmarks"],
            "Exact ignored Linux acceptance test was not uniquely listed")


def compiled_test(data):
    require(len(data) <= MAX_LOG_BYTES, "Compiler output exceeds bound")
    executables = []
    for line in data.decode("utf-8", errors="strict").splitlines():
        if not line.startswith("{"):
            continue
        event = strict_json(line)
        if event.get("reason") == "compiler-artifact" and event.get("target", {}).get("name") == "cedar_app" \
                and event.get("profile", {}).get("test") is True and event.get("executable"):
            executables.append(event["executable"])
    require(len(executables) == 1 and type(executables[0]) is str,
            "Expected one exact cedar-app test executable")
    return Path(executables[0]).resolve(strict=True)


def test_compile_command():
    # Shared semantic fixtures are feature-gated; this builds only the app test
    # harness. The caller-supplied default-feature agent remains hash-checked.
    return ["cargo", "test", "-p", "cedar-app", "--lib", "--locked", "--offline",
            "--features", "windows-language-validation", "--no-run", "--message-format=json"]


def run(root, scratch_root, java, agent, existing_archive=None):
    require(sys.platform == "linux" and platform.machine() == "x86_64", "Native Linux x86_64 is required")
    root = Path(root).resolve(strict=True)
    scratch_root = Path(scratch_root).resolve(strict=True)
    java = Path(java).resolve(strict=True)
    agent = Path(agent).resolve(strict=True)
    require(java.name == "java" and java.is_file() and os.access(java, os.X_OK), "Existing Java executable is required")
    require(agent.name == "cedar-agent" and agent.is_file() and os.access(agent, os.X_OK), "Prebuilt normal cedar-agent is required")
    agent_digest = hashlib.sha256(read_regular(agent, MAX_AGENT_BYTES)).digest()
    release = read_regular(java.parent.parent / "release", 65536).decode("utf-8", errors="strict")
    require(re.search(r'^JAVA_VERSION="21(?:[.\-+][^"\r\n]*)?"$', release, re.MULTILINE), "Existing JDK 21 is required")
    require(scratch_root.is_dir(), "Scratch root must exist")
    evidence_path = scratch_root / EVIDENCE_NAME
    require(not evidence_path.exists(), "Sanitized evidence destination must be fresh")
    scratch = Path(tempfile.mkdtemp(prefix="cedar-linux-java-", dir=scratch_root))
    stage = "prepare"
    runtime_log = None
    preparation_log = scratch / "preparation-private.log"
    try:
        environment = os.environ.copy()
        for key in JAVA_ENVIRONMENT_KEYS:
            environment.pop(key, None)
        environment["JAVA_HOME"] = str(java.parent.parent)
        require(all(key not in environment for key in JAVA_ENVIRONMENT_KEYS),
                "Java launcher environment was not cleared")
        commit_log = scratch / "source-commit-private.log"
        dirty_log = scratch / "source-dirty-private.log"
        bounded_process(["git", "rev-parse", "--verify", "HEAD"], root, environment, commit_log, 10)
        source_commit = read_regular(commit_log, 128).decode("ascii").strip()
        require(re.fullmatch(r"[0-9a-f]{40}", source_commit), "Exact source commit is required")
        bounded_process(["git", "status", "--porcelain", "--untracked-files=normal"],
                        root, environment, dirty_log, 10)
        checkout_dirty = bool(read_regular(dirty_log, MAX_LOG_BYTES))
        archive = scratch / "jdtls-1.61.0.tar.gz"
        if existing_archive is not None:
            data = read_regular(Path(existing_archive), MAX_ARCHIVE_BYTES)
            require(hashlib.sha256(data).hexdigest() == ARCHIVE_SHA256, "Existing archive checksum mismatch")
            disk_preflight(shutil.disk_usage(scratch).free, len(data))
            with archive.open("xb") as stream:
                stream.write(data)
        distribution = scratch / "JDT distribution 雪"
        bounded_process([sys.executable, str(Path(__file__).resolve()), "--prepare-archive", str(archive),
                         "--prepare-distribution", str(distribution)], root, environment,
                        preparation_log, PREPARE_TIMEOUT)
        stage = "jdk"
        bounded_process([str(java), "-version"], scratch, environment, scratch / "jdk-private.log", 15)
        environment.update(CEDAR_JAVA=str(java), CEDAR_JDTLS_HOME=str(distribution), CEDAR_AGENT_BIN=str(agent))
        # Keep OS temp files, JDT data, and JVM crash reports in this private tree.
        environment["TMPDIR"] = str(scratch)
        stage = "compile"
        compile_log = scratch / "compile-private.log"
        bounded_process(test_compile_command(), root, environment, compile_log, COMPILE_TIMEOUT)
        require(hashlib.sha256(read_regular(agent, MAX_AGENT_BYTES)).digest() == agent_digest,
                "Compilation changed the prebuilt normal agent")
        executable = compiled_test(read_regular(compile_log, MAX_LOG_BYTES))
        require(executable.is_relative_to(root / "target") and executable.is_file(),
                "Compiler returned an unexpected test executable")
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
        require(hashlib.sha256(read_regular(agent, MAX_AGENT_BYTES)).digest() == agent_digest,
                "Acceptance changed the prebuilt normal agent")
        stage = "cleanup"
        shutil.rmtree(scratch)
        require(not scratch.exists(), "Private acceptance scratch cleanup failed")
    except Exception as error:
        # Do not print exception messages: transport/Java errors can contain paths,
        # source, stderr, command lines, or inherited environment values.
        classification, sanitized = failure_probe(runtime_log)
        failure = {"schema_version": 1, "kind": "linux_java_acceptance", "status": "failed", "stage": stage,
                   "raw_logs_published": False, "private_scratch_retained": scratch.exists(),
                   "probe_record_status": classification}
        if sanitized is not None:
            failure["probe"] = sanitized
        disk_failed = isinstance(error, DiskPreflightError)
        if stage == "prepare" and preparation_log.exists():
            try:
                disk_failed |= DISK_FAILURE_MARKER in read_regular(preparation_log, MAX_LOG_BYTES).splitlines()
            except (ValueError, OSError):
                pass
        if disk_failed:
            failure["category"] = "disk_preflight"
        publish(evidence_path, failure)
        raise RuntimeError("Linux Java acceptance failed; raw details remain private") from None
    result = {"schema_version": 1, "kind": "linux_java_acceptance", "status": "success",
              "jdt_version": "1.61.0", "jdt_archive_sha256": ARCHIVE_SHA256,
              "pinned_archive_verified": True, "existing_jdk21_verified": True,
              "normal_agent_normal_client": True, "normal_agent_unchanged": True,
              "source_commit": source_commit, "checkout_dirty": checkout_dirty,
              "source_snapshot": "before_preparation",
              "agent_sha256": agent_digest.hex(), "agent_build_provenance": "caller_supplied_prebuilt",
              "agent_source_equivalence_verified": False,
              "launcher_environment_cleared": True, "launcher_environment_keys_checked": 6,
              "shutdown_evidence": "backend_owned_linux_stop",
              "preparation_timeout_s": PREPARE_TIMEOUT,
              "compile_timeout_s": COMPILE_TIMEOUT, "test_watchdog_s": TEST_TIMEOUT,
              "exact_test_selection_verified": True, "selection_timeout_s": SELECTION_TIMEOUT,
              "scratch_removed": True, "raw_logs_published": False, "gui_exercised": False,
              "maven_exercised": False, "network_isolation_verified": False, "probe": probe}
    publish(evidence_path, result)
    return result


def publish(path, result):
    encoded = json.dumps(result, sort_keys=True)
    with Path(path).open("x", encoding="utf-8") as stream:
        stream.write(encoded + "\n")
    print(encoded)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scratch-root", default=os.environ.get("RUNNER_TEMP"))
    parser.add_argument("--java", default=os.environ.get("CEDAR_JAVA") or
                        (str(Path(os.environ["JAVA_HOME"]) / "bin/java") if "JAVA_HOME" in os.environ else None))
    parser.add_argument("--agent")
    parser.add_argument("--archive")
    parser.add_argument("--prepare-archive", help=argparse.SUPPRESS)
    parser.add_argument("--prepare-distribution", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.prepare_archive or args.prepare_distribution:
        require(args.prepare_archive and args.prepare_distribution, "Both preparation paths are required")
        try:
            prepare(args.prepare_archive, args.prepare_distribution)
        except DiskPreflightError:
            print(DISK_FAILURE_MARKER.decode("ascii"))
            raise
        return
    root = Path(__file__).resolve().parent.parent
    require(args.scratch_root and args.java, "Explicit scratch root and existing JDK 21 are required")
    run(root, args.scratch_root, args.java, args.agent or root / "target/release/cedar-agent", args.archive)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # This CLI boundary must also sanitize preparation and early validation.
        print("Linux Java acceptance did not complete.", file=sys.stderr)
        sys.exit(1)
