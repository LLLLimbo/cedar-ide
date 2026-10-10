#!/usr/bin/env python3
"""Verify the packaged Linux desktop's fixed-sibling client route, headlessly.

Requires existing normal release cedar/cedar-agent and the opt-in nonshipping
cedar-client-bundle-probe. This script never downloads, opens SSH/network, starts
GUI rendering, or executes language/task tools. It does not attest to native GUI
startup. The CI enclosing timeout is a failure boundary, never cleanup evidence.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib

import package_linux_desktop_bundle as bundle

PROBE_NAME = "cedar-client-bundle-probe"
ROOT = "workspace café 雪"
DIRECTORY = "files café 雪"
FILE = DIRECTORY + "/saved file λ.txt"
MARKER = ".cedar-linux-desktop-acceptance"
ORIGINAL = "cedar-linux-bundle-original-v1 café 雪\n".encode()
SAVED = "cedar-linux-bundle-saved-v1 café 雪\n".encode()
FIXTURE_FILES = {
    ROOT + "/" + MARKER: b"cedar-linux-desktop-acceptance-v1\n",
    ROOT + "/source preserved.txt": b"cedar-linux-source-preserved-v1\n",
    "outside preserved.txt": b"cedar-linux-outside-preserved-v1\n",
    ROOT + "/" + FILE: ORIGINAL,
}
BOOLS = (
    "linux_x86_64", "bundled_linux_connection", "fixed_sibling_agent",
    "package_version_matches", "capabilities_exact", "trust_off", "list_verified",
    "read_verified", "conditional_write_verified", "readback_verified", "search_verified",
    "stale_write_rejected", "root_escape_rejected", "task_operations_rejected",
    "language_operations_rejected", "git_operations_rejected", "typed_java_advertised",
    "maven_groups_advertised", "maven_operations_rejected", "errors_leave_client_usable",
    "preserved_fixture_unchanged", "only_expected_file_changed", "reconnect_saved_bytes",
    "initial_client_reaped", "reconnect_client_reaped",
)
FIXED = {
    "schema_version": 1, "protocol_version": 4, "agent_info_schema": 1,
    "capability_count": 31, "capability_group_count": 2,
    "agent_processes_spawned": 2, "elapsed_bound_ms": 30000,
    "client_call_limit": 96, "agent_spawn_limit": 2, "close_bound_ms": 5000,
}
REJECTION_BOOLS = (
    "fixed_sibling_only", "misleading_path_and_cwd_ignored", "trust_off",
    "controlled_fixtures_only", "fixtures_removed",
)
REJECTION_FIXED = {
    "schema_version": 1, "cases": 20, "pre_spawn_cases": 10,
    "reaped_child_cases": 10, "elapsed_bound_ms": 90000, "per_probe_bound_ms": 15000,
}
TEST_NAME = "linux_desktop_fixed_sibling_rejects_controlled_failures"
MAX_TEST_OUTPUT = 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate receipt key")
        result[key] = value
    return result


def validate_receipt(receipt, kind, booleans, fixed, variable):
    require(type(receipt) is dict, "Probe receipt must be an object")
    require(set(receipt) == set(booleans) | set(fixed) | set(variable) | {"kind", "status"},
            "Unexpected probe receipt schema")
    require(receipt["kind"] == kind and receipt["status"] == "success",
            "Probe did not succeed")
    for key in booleans:
        require(receipt[key] is True, "Incomplete probe witness")
    for key, expected in fixed.items():
        require(type(receipt[key]) is int and receipt[key] == expected,
                "Unexpected probe bound or identity")
    for key in variable:
        require(type(receipt[key]) is int, "Noninteger probe count")
    require(0 <= receipt["elapsed_ms"] <= receipt["elapsed_bound_ms"],
            "Probe exceeded elapsed bound")
    return receipt


def validate_probe(receipt):
    validate_receipt(receipt, "cedar_linux_desktop_bundle_probe", BOOLS, FIXED,
                     {"elapsed_ms", "explicit_client_calls"})
    require(1 <= receipt["explicit_client_calls"] <= receipt["client_call_limit"],
            "Invalid request count")
    return receipt


def validate_rejections(receipt):
    return validate_receipt(receipt, "cedar_linux_desktop_rejection_suite", REJECTION_BOOLS,
                            REJECTION_FIXED, {"elapsed_ms"})


def parse_receipt(data, validator):
    require(type(data) is bytes and len(data) <= MAX_TEST_OUTPUT,
            "Probe output exceeds bound")
    text = data.decode("utf-8", errors="strict")
    objects = []
    for line in text.splitlines():
        if line.startswith("{"):
            objects.append(json.loads(line, object_pairs_hook=unique_object,
                                      parse_constant=lambda _: (_ for _ in ()).throw(
                                          ValueError("Nonfinite receipt number"))))
    require(len(objects) == 1, "Expected one probe receipt")
    return validator(objects[0])


def parse_probe(data):
    return parse_receipt(data, validate_probe)


def fixture_snapshot(directory):
    expected_directories = {ROOT, ROOT + "/" + DIRECTORY}
    found = {}
    for path in directory.rglob("*"):
        require(not path.is_symlink(), "Generated fixture contains a symlink")
        relative = path.relative_to(directory).as_posix()
        if path.is_dir():
            require(relative in expected_directories, "Unexpected generated directory")
        else:
            require(relative in FIXTURE_FILES, "Unexpected generated file")
            found[relative] = bundle.read_regular(path, 4096)
    return found


def run_process(command, cwd, log, timeout):
    # Bounded nonblocking drain: at most one overflow byte is observed and no
    # byte beyond MAX_TEST_OUTPUT reaches the private file. The deadline also
    # covers draining output after direct-child exit. No background reader,
    # process-group signaling, PID search, or global reaper is involved.
    started = time.monotonic()
    total = 0
    with log.open("xb") as stream:
        process = subprocess.Popen(command, cwd=cwd, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        try:
            require(process.stdout is not None, "Probe output pipe missing")
            os.set_blocking(process.stdout.fileno(), False)
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                open_pipe = True
                while open_pipe or process.poll() is None:
                    remaining = timeout - (time.monotonic() - started)
                    require(remaining > 0, "Probe timed out; agent cleanup unverified")
                    for key, _ in selector.select(min(remaining, 0.1)):
                        chunk = os.read(key.fd, min(65536, MAX_TEST_OUTPUT - total + 1))
                        if not chunk:
                            selector.unregister(process.stdout)
                            open_pipe = False
                            continue
                        require(len(chunk) <= MAX_TEST_OUTPUT - total,
                                "Probe output exceeded bound; agent cleanup unverified")
                        stream.write(chunk)
                        total += len(chunk)
                require(process.returncode == 0, "Probe process failed")
                require(time.monotonic() - started < timeout,
                        "Probe returned after deadline; agent cleanup unverified")
        finally:
            # Killing this owned direct subprocess is only a failure boundary.
            # It cannot establish that its Client-owned agent was reaped.
            try:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
            except (OSError, subprocess.TimeoutExpired):
                raise ValueError("Probe supervisor cleanup unverified") from None
            finally:
                if process.stdout is not None:
                    process.stdout.close()
    return bundle.read_regular(log, MAX_TEST_OUTPUT)


def run(root, scratch_root, source_commit, ci_run_url):
    # Actual Ubuntu 24.04 amd64 is required before any output or fixture exists.
    bundle.require_build_host()
    require(type(source_commit) is str and bundle.HEX_COMMIT.fullmatch(source_commit),
            "Exact source commit is required")
    require(type(ci_run_url) is str and bundle.RUN_URL.fullmatch(ci_run_url),
            "Exact project CI URL is required")
    root = Path(root).resolve(strict=True)
    scratch_root = bundle.checked_path(scratch_root)
    require(scratch_root.is_dir(), "Scratch root must exist")
    output = scratch_root / "cedar-linux-desktop-development"
    require(not output.exists(), "Output directory already exists")
    output.mkdir(mode=0o700)
    scratch = Path(tempfile.mkdtemp(prefix="cedar-linux-desktop-", dir=scratch_root))
    completed = False
    try:
        version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        require(bundle.VERSION.fullmatch(version) is not None, "Invalid source version")
        archive = output / f"cedar-{version}-linux-x86_64-{source_commit[:12]}.tar.gz"
        built = bundle.build(root, root / "target/release", archive, source_commit, ci_run_url)
        data = bundle.read_regular(archive, bundle.MAX_ARCHIVE_BYTES)
        manifest, payload = bundle.verify_bytes(data, source_commit, ci_run_url)
        bundle.verify_source(root, manifest, payload)
        require(PROBE_NAME not in payload, "The test probe must never ship")
        extracted = scratch / "renamed Cedar desktop 雪 with spaces"
        bundle.extract_payload(payload, extracted)
        bundle.verify_extracted(payload, extracted)
        probe_source = root / "target/release" / PROBE_NAME
        probe_bytes = bundle.read_regular(probe_source, bundle.MAX_BINARY_BYTES)
        require(probe_bytes.startswith(b"\x7fELF\x02\x01\x01"), "Native test probe is required")
        probe = extracted / PROBE_NAME
        with probe.open("xb") as stream:
            stream.write(probe_bytes)
        probe.chmod(0o755)
        require(bundle.read_regular(probe, bundle.MAX_BINARY_BYTES) == probe_bytes,
                "Test probe copy changed")
        fixture = scratch / "generated fixture only"
        workspace = fixture / ROOT
        (workspace / DIRECTORY).mkdir(parents=True, mode=0o700)
        for relative, contents in FIXTURE_FILES.items():
            with (fixture / relative).open("xb") as stream:
                stream.write(contents)
        require(fixture_snapshot(fixture) == FIXTURE_FILES, "Generated fixture differs")
        probe_receipt = parse_probe(run_process(
            [str(probe), "portable", str(workspace)], scratch,
            scratch / "portable-private.log", timeout=45,
        ))
        expected_after = {**FIXTURE_FILES, ROOT + "/" + FILE: SAVED}
        require(fixture_snapshot(fixture) == expected_after, "Generated fixture changed unexpectedly")
        require(bundle.read_regular(probe, bundle.MAX_BINARY_BYTES) == probe_bytes,
                "Test probe changed during acceptance")
        probe.unlink()
        bundle.verify_extracted(payload, extracted)
        # Only generated safe peers are used by this separately built suite.
        # It invokes the same opt-in probe beside deliberately invalid siblings.
        rejection_receipt = parse_receipt(run_process(
            ["cargo", "test", "--release", "-p", "cedar-client", "--features", "fixtures",
             "--test", "linux_desktop_bundle", "--locked", "--offline", "--", "--exact",
             TEST_NAME, "--nocapture", "--test-threads=1"],
            root, scratch / "negative-private.log", timeout=180,
        ), validate_rejections)
        bundle.verify_extracted(payload, extracted)
        require(bundle.read_regular(archive, bundle.MAX_ARCHIVE_BYTES) == data,
                "Archive changed during probe")
        bundle.verify_source(root, manifest, payload)
        completed = True
    finally:
        shutil.rmtree(scratch)
        require(not scratch.exists(), "Generated scratch removal failed")
    require(completed, "Bundle acceptance incomplete")
    binaries = {name: {"bytes": len(payload[name]),
                       "sha256": hashlib.sha256(payload[name]).hexdigest()}
                for name in bundle.BINARIES}
    result = {
        "schema_version": 1, "kind": "cedar_linux_desktop_development_bundle",
        "status": "success", "version": version,
        "source_commit": source_commit, "ci_run_url": ci_run_url,
        "archive_name": archive.name, "archive_bytes": len(data),
        "archive_sha256": hashlib.sha256(data).hexdigest(),
        "payload_count": len(payload) - 1, "binaries": binaries, "abi": built["abi"],
        "unsigned": True, "source_documents_verified": True,
        "ubuntu_24_04_amd64_host_verified": True, "unicode_extraction_verified": True,
        "executable_mode_verified": True, "payload_unchanged": True,
        "probe_excluded_from_archive": True, "synthetic_root_removed": True,
        "scratch_removed": True, "authenticated_ssh_exercised": False,
        "network_exercised": False, "gui_exercised": False,
        "native_desktop_acceptance": False, "deployment_performed": False,
        "probe": probe_receipt, "rejections": rejection_receipt,
    }
    with (output / "BUNDLE_VERIFICATION.json").open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(result, sort_keys=True, ensure_ascii=False))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scratch-root", default=os.environ.get("RUNNER_TEMP"))
    parser.add_argument("--source-commit", default=os.environ.get("GITHUB_SHA"))
    parser.add_argument("--ci-run-url", default="https://github.com/LLLLimbo/cedar-ide/actions/runs/"
                        + os.environ.get("GITHUB_RUN_ID", ""))
    args = parser.parse_args()
    require(args.scratch_root is not None, "Scratch root is required")
    run(Path(__file__).resolve().parent.parent, args.scratch_root, args.source_commit, args.ci_run_url)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, bundle.BundleError):
        # Never print raw Cargo, filesystem, protocol, or environment details.
        print("linux-desktop-acceptance-error:verification_failed", file=sys.stderr)
        sys.exit(1)
