#!/usr/bin/env python3
"""Run two exact Browse proofs using the aggregate CI feature graph.

Artifact compilation (120 s), backend runtime (180 s), and app runtime (330 s)
have independent outer bounds. Native 5 s backend close and 240/30/300 s app
budgets remain unchanged. Driver termination never attests to agent cleanup.
"""
import hashlib
import os
from pathlib import Path
import re
import sys

from copy_draft_acceptance import (
    AcceptanceError, MAX_COMPILE_BYTES, MAX_RUNTIME_BYTES,
    decode_output, require, strict_json, typed_equal,
)
from report_triage_diagnostics import CapturedFailure, bounded_process

BACKEND_TEST = "file_discovery_normal_agent_finds_untouched_binary_and_large_paths_without_trust"
APP_TEST = "browse_tests::browse_normal_agent_find_recent_project_reveal_continuous_preview_and_exact_return"
PREFIX = "browse_normal_acceptance "
COMPILE_TIMEOUT = 120
BACKEND_TIMEOUT = 180
APP_TIMEOUT = 330
TARGET = "untouched/deep/Needle.rs"


def compile_command():
    return ["cargo", "test", "--workspace", "--all-targets", "--all-features",
            "--locked", "--no-run", "--message-format=json"]


def select_artifacts(data, root, platform):
    require(platform in ("linux", "windows"), "unsupported_platform")
    specifications = {
        "stdio_roundtrip": ("client", ["test"], "tests/stdio_roundtrip.rs"),
        "cedar_app": ("app", ["lib"], "src/lib.rs"),
    }
    selected = {}
    for line in decode_output(data, MAX_COMPILE_BYTES).splitlines():
        if not line.startswith("{"):
            continue
        event = strict_json(line)
        require(type(event) is dict, "invalid_compiler_event")
        if event.get("reason") != "compiler-artifact":
            continue
        target = event.get("target")
        require(type(target) is dict, "invalid_compiler_target")
        name = target.get("name")
        require(type(name) is str, "invalid_compiler_target")
        if name not in specifications:
            continue
        profile = event.get("profile")
        require(type(profile) is dict and type(profile.get("test")) is bool,
                "invalid_test_profile")
        if not profile["test"]:
            continue
        crate, kind, source = specifications[name]
        require(target.get("kind") == kind
                and event.get("manifest_path") == str(root / "crates" / crate / "Cargo.toml")
                and target.get("src_path") == str(root / "crates" / crate / source),
                "unexpected_test_source")
        require(name not in selected, "duplicate_test_artifact")
        value = event.get("executable")
        require(type(value) is str, "missing_test_executable")
        executable = Path(value)
        suffix = r"\.exe" if platform == "windows" else ""
        require(executable.is_absolute() and executable.parent == root / "target/debug/deps"
                and re.fullmatch(name + r"-[0-9a-f]+" + suffix, executable.name)
                and executable.is_file() and not executable.is_symlink()
                and executable.resolve(strict=True) == executable, "unexpected_test_binary")
        selected[name] = executable
    require(selected.keys() == specifications.keys(), "exact_two_test_artifacts_required")
    return selected


def expected_receipt():
    return {
        "receipt_version": 5,
        "operation_ledger": [
            {"op": "Hello"}, {"op": "List", "path": ""},
            {"op": "Read", "path": "origin.txt"},
            {"op": "FindFiles", "query": "Needle.rs", "scope": "", "limit": 100},
            {"op": "Read", "path": TARGET}, {"op": "Read", "path": TARGET},
            {"op": "Search", "query": "needle", "limit": 500},
            {"op": "Read", "path": TARGET},
            {"op": "List", "path": ""},
            {"op": "List", "path": "untouched"},
            {"op": "List", "path": "untouched/deep"},
            {"op": "List", "path": ""},
            {"op": "Read", "path": TARGET},
            {"op": "Read", "path": TARGET},
        ],
        "source_sha256": [
            ["origin.txt", hashlib.sha256("saved Ω😀\r\nneedle second\nlast\n".encode()).hexdigest()],
            [TARGET, hashlib.sha256(b"first\nneedle target\nlast\n").hexdigest()],
        ],
        "connections": 1, "agents_reaped": 1, "trust": False,
        "active_budget_seconds": 240, "cleanup_timeout_seconds": 30,
        "watchdog_seconds": 300, "dirty_preview_and_open_additional_reads": 0,
        "unopened_preview_reads": 1, "unopened_open_reads": 1,
        "retained_text_snapshot": True,
        "grouped_original_indices": True,
        "text_open_back_exact": True,
        "reopen_keeps_selection": True,
        "grouped_navigation_additional_operations": 0,
        "retained_dirty_preview_additional_reads": 0,
        "shell_project_focus_retains_find": True,
        "shell_find_reopen_active_hit": True,
        "shell_hide_exact_editor_return": True,
        "shell_state_preserved": True,
        "shell_transition_operations": 0,
        "recent_popup_filter_select_operations": 0,
        "recent_closed_reopen_reads": 1,
        "recent_closed_reopen_normal_position": True,
        "recent_live_dirty_reuse_reads": 0,
        "recent_live_dirty_reuse_exact": True,
        "recent_find_state_preserved": True,
        "project_reveal_ancestor_lists": 3,
        "project_reveal_additional_reads": 0,
        "project_reveal_cached_operations": 0,
        "project_reveal_selected_and_focused": True,
        "project_reveal_state_preserved": True,
        "project_reveal_cancelled_lists": 1,
        "project_reveal_cancelled_followup_operations": 0,
        "project_reveal_intent_timeout_seconds": 30,
        "project_reveal_max_ancestor_lists": 32,
        "continuous_preview_unarmed_operations": 0,
        "continuous_preview_browse_armed_follow": True,
        "continuous_preview_bottom_armed_follow": True,
        "continuous_preview_dirty_additional_reads": 0,
        "continuous_preview_latest_candidate": True,
        "continuous_preview_unopened_reads": 1,
        "continuous_preview_open_reads": 1,
        "continuous_preview_open_back_exact": True,
        "continuous_preview_state_preserved": True,
        "continuous_preview_debounce_ms": 150,
        "continuous_preview_max_context_lines": 41,
        "continuous_preview_max_context_bytes": 64 * 1024,
        "current_file_find_selection_seeded": True,
        "current_file_find_next_previous_wrap": True,
        "current_file_find_literal_no_match": True,
        "current_file_find_escape_back_exact": True,
        "current_file_find_undo_redo_preserved": True,
        "current_file_find_state_preserved": True,
        "current_file_find_additional_operations": 0,
    }


def validate_execution(data, name):
    text = decode_output(data, MAX_RUNTIME_BYTES)
    lines = text.splitlines()
    require([line for line in lines if line.startswith("running ")] == ["running 1 test"],
            "exact_one_test_not_executed")
    results = [line for line in lines if line.startswith("test result:")]
    require(len(results) == 1 and re.fullmatch(
        r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; "
        r"[0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s", results[0]),
        "exact_one_test_not_executed")
    executions = [line for line in lines if line.startswith("test ")
                  and not line.startswith("test result:")]
    require(len(executions) == 1 and executions[0].startswith("test " + name + " ... "),
            "selected_test_execution_missing")
    if name == APP_TEST:
        require(text.count(PREFIX) == 1, "exact_one_receipt_required")
        marked = next(line for line in lines if PREFIX in line)
        before, value = marked.split(PREFIX, 1)
        require(before in ("", "test " + name + " ... "), "invalid_receipt_prefix")
        require(typed_equal(strict_json(value), expected_receipt()), "fixed_receipt_contract_mismatch")


def checked_stage(stage, command, root, environment, timeout, limit, validator, runner):
    try:
        output = runner(command, root, environment, timeout, limit)
    except CapturedFailure as error:
        print(error.output.decode("utf-8", errors="replace"), end="", file=sys.stderr)
        raise AcceptanceError(
            f"{stage}:{error}; returncode={error.returncode}; "
            f"direct_child_cleanup={error.cleanup}; native_cleanup=unverified"
        ) from None
    try:
        result = validator(output)
    except AcceptanceError as error:
        print(output.decode("utf-8", errors="replace"), end="", file=sys.stderr)
        raise AcceptanceError(f"{stage}:{error}; native_cleanup=unverified") from None
    if stage != "compile":
        print(output.decode("utf-8"), end="")
    return result


def run(root, runner=bounded_process):
    root = Path(root).resolve(strict=True)
    platform = {"linux": "linux", "win32": "windows"}.get(sys.platform)
    require(platform is not None, "unsupported_platform")
    agent = root / "target/release" / ("cedar-agent.exe" if platform == "windows" else "cedar-agent")
    require(agent.is_file() and not agent.is_symlink() and agent.resolve(strict=True) == agent,
            "normal_release_agent_missing")
    environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
    artifacts = checked_stage("compile", compile_command(), root, environment,
                              COMPILE_TIMEOUT, MAX_COMPILE_BYTES,
                              lambda data: select_artifacts(data, root, platform), runner)
    for stage, target, name, variable, timeout in (
        ("backend", "stdio_roundtrip", BACKEND_TEST, "CEDAR_AGENT_BIN", BACKEND_TIMEOUT),
        ("app", "cedar_app", APP_TEST, "CEDAR_BROWSE_AGENT_BIN", APP_TIMEOUT),
    ):
        command = [str(artifacts[target]), name, "--ignored", "--exact", "--test-threads=1", "--nocapture"]
        checked_stage(stage, command, root, dict(environment, **{variable: str(agent)}),
                      timeout, MAX_RUNTIME_BYTES, lambda data: validate_execution(data, name), runner)


if __name__ == "__main__":
    try:
        run(Path(__file__).resolve().parents[1])
    except (AcceptanceError, OSError) as error:
        sys.exit(f"Browse process acceptance failed: {error}")
