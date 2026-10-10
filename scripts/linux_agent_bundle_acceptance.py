#!/usr/bin/env python3
"""Verify an extracted normal Linux agent package; no SSH or deployment.

The CI step's enclosing timeout is a failure boundary, never a cleanup receipt.
The exact ignored Rust test retains ordinary Client request/close ownership.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

import package_linux_agent_bundle as bundle

BOOLS = (
    "linux_x86_64", "extracted_agent_stdio", "package_version_matches",
    "capabilities_exact", "trust_off", "list_verified", "read_verified",
    "conditional_write_verified", "readback_verified", "search_verified",
    "stale_write_rejected", "root_escape_rejected", "task_operations_rejected",
    "language_operations_rejected", "git_operations_rejected",
    "typed_java_advertised", "maven_groups_advertised", "maven_operations_rejected",
    "errors_leave_client_usable",
    "preserved_fixture_unchanged", "only_expected_file_changed",
    "reconnect_saved_bytes", "initial_client_reaped", "reconnect_client_reaped",
    "fixture_removed",
)
FIXED = {
    "schema_version": 1, "protocol_version": 4, "agent_info_schema": 1,
    "capability_count": 31, "capability_group_count": 2,
    "agent_processes_spawned": 2, "elapsed_bound_ms": 30000,
    "client_call_limit": 96, "agent_spawn_limit": 2, "close_bound_ms": 5000,
}
VARIABLE = {"explicit_client_calls", "elapsed_ms"}
TEST_NAME = "extracted_linux_agent_stdio_acceptance"
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


def validate_probe(receipt):
    require(type(receipt) is dict, "Probe receipt must be an object")
    require(set(receipt) == set(BOOLS) | set(FIXED) | VARIABLE | {"kind", "status"},
            "Unexpected probe receipt schema")
    require(receipt["kind"] == "cedar_linux_agent_bundle_probe"
            and receipt["status"] == "success", "Probe did not succeed")
    for key in BOOLS:
        require(receipt[key] is True, "Incomplete probe witness")
    for key, expected in FIXED.items():
        require(type(receipt[key]) is int and receipt[key] == expected,
                "Unexpected probe bound or identity")
    for key in VARIABLE:
        require(type(receipt[key]) is int, "Noninteger probe count")
    require(1 <= receipt["explicit_client_calls"] <= receipt["client_call_limit"],
            "Invalid request count")
    require(0 <= receipt["elapsed_ms"] <= receipt["elapsed_bound_ms"],
            "Probe exceeded elapsed bound")
    return receipt


def parse_probe(data):
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
    return validate_probe(objects[0])


def run(root, scratch_root, source_commit, ci_run_url):
    bundle.require_build_host()
    require(type(source_commit) is str and bundle.HEX_COMMIT.fullmatch(source_commit),
            "Exact source commit is required")
    require(type(ci_run_url) is str and bundle.RUN_URL.fullmatch(ci_run_url),
            "Exact project CI URL is required")
    root = Path(root).resolve(strict=True)
    scratch_root = bundle.checked_path(scratch_root)
    require(scratch_root.is_dir(), "Scratch root must exist")
    output = scratch_root / "cedar-linux-agent-development"
    require(not output.exists(), "Output directory already exists")
    output.mkdir(mode=0o700)
    scratch = Path(tempfile.mkdtemp(prefix="cedar-linux-agent-", dir=scratch_root))
    completed = False
    try:
        version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        require(bundle.VERSION.fullmatch(version) is not None, "Invalid source version")
        archive = output / f"cedar-agent-{version}-linux-x86_64-{source_commit[:12]}.tar.gz"
        built = bundle.build(root, root / "target/release", archive, source_commit, ci_run_url)
        data = bundle.read_regular(archive, bundle.MAX_ARCHIVE_BYTES)
        manifest, payload = bundle.verify_bytes(data, source_commit, ci_run_url)
        bundle.verify_source(root, manifest, payload)
        extracted = scratch / "extracted agent 雪 with spaces"
        bundle.extract_payload(payload, extracted)
        bundle.verify_extracted(payload, extracted)
        environment = os.environ.copy()
        environment["CEDAR_LINUX_BUNDLE_AGENT_BIN"] = str(extracted / bundle.AGENT)
        # Raw Cargo/test output remains only inside this generated scratch tree.
        # The Rust test uses a fixed semantic budget plus unchanged Client RPC
        # deadlines and consumes every established Client on its failure paths.
        log = scratch / "probe-private.log"
        with log.open("xb") as stream:
            outcome = subprocess.run(
                ["cargo", "test", "-p", "cedar-client", "--test", "linux_agent_bundle",
                 "--locked", "--offline", "--", "--ignored", "--exact", TEST_NAME,
                 "--nocapture", "--test-threads=1"],
                cwd=root, env=environment, stdin=subprocess.DEVNULL,
                stdout=stream, stderr=subprocess.STDOUT, check=False,
            )
        require(outcome.returncode == 0, "Extracted agent probe failed")
        probe = parse_probe(bundle.read_regular(log, MAX_TEST_OUTPUT))
        bundle.verify_extracted(payload, extracted)
        require(bundle.read_regular(archive, bundle.MAX_ARCHIVE_BYTES) == data,
                "Archive changed during probe")
        bundle.verify_source(root, manifest, payload)
        completed = True
    finally:
        shutil.rmtree(scratch)
        require(not scratch.exists(), "Generated scratch removal failed")
    require(completed, "Bundle acceptance incomplete")
    result = {
        "schema_version": 1, "kind": "cedar_linux_agent_development_bundle",
        "status": "success", "version": version,
        "source_commit": source_commit, "ci_run_url": ci_run_url,
        "archive_name": archive.name, "archive_bytes": len(data),
        "archive_sha256": hashlib.sha256(data).hexdigest(),
        "payload_count": len(payload) - 1, "abi": built["abi"],
        "source_documents_verified": True, "unicode_extraction_verified": True,
        "executable_mode_verified": True, "payload_unchanged": True,
        "scratch_removed": True, "authenticated_ssh_exercised": False,
        "gui_exercised": False, "deployment_performed": False,
        "probe": probe,
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
    require(type(args.source_commit) is str and len(args.source_commit) == 40
            and all(c in "0123456789abcdef" for c in args.source_commit),
            "Exact source commit is required")
    require(bundle.RUN_URL.fullmatch(args.ci_run_url) is not None, "Exact project CI URL is required")
    run(Path(__file__).resolve().parent.parent, args.scratch_root, args.source_commit, args.ci_run_url)


if __name__ == "__main__":
    main()
