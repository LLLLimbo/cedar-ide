#!/usr/bin/env python3
"""Real normal-agent/real-Git acceptance using only owned synthetic repositories.

Usage: python scripts/git_views_smoke.py --agent ABSOLUTE_AGENT --git ABSOLUTE_GIT
Requires Git >= 2.45 and Linux (pidfd support) or Windows. No installs, network,
user repositories, GUI Trust, SSH, or Rust fixtures. Git setup uses only init,
config, add and commit in temporary repositories with a fixed synthetic identity.

Execution trust is essential: Git clean filters really execute programs. These
tests deliberately configure a generated Python filter, including its writes and
descendants, inside the owned temporary tree. This is not evidence that filters
in other repositories are harmless or that read-only Git actions are a sandbox.

Public output contains fixed classifications, booleans and counts only. Raw Git,
agent, task and filter output, paths, identities and environment stay private.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import select
import shlex
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
import uuid
import zlib


MAX_FRAME = 8 * 1024 * 1024
REQUEST_SECONDS = 16
STAGE = "arguments"
ASSERTIONS = 0


class Failure(Exception):
    """Only fixed, locally supplied codes may enter public diagnostics."""


def require(condition, code):
    global ASSERTIONS
    ASSERTIONS += 1
    if not condition:
        raise Failure(code)


def clean_environment():
    result = {key: value for key, value in os.environ.items()
              if not key.upper().startswith(("GIT_", "PYTHON"))
              and not key.startswith("=")}
    result.update({
        "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_SYSTEM": os.devnull,
        "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_COUNT": "0",
        "GIT_OPTIONAL_LOCKS": "0", "GIT_NO_LAZY_FETCH": "1",
        "GIT_NO_REPLACE_OBJECTS": "1", "GIT_LITERAL_PATHSPECS": "1",
        "GIT_TERMINAL_PROMPT": "0", "GIT_ALLOW_PROTOCOL": "",
        "GIT_ATTR_NOSYSTEM": "1", "LC_ALL": "C", "LANG": "C",
        "LANGUAGE": "C", "GIT_AUTHOR_NAME": "Cedar Synthetic Acceptance",
        "GIT_AUTHOR_EMAIL": "cedar-synthetic@example.invalid",
        "GIT_COMMITTER_NAME": "Cedar Synthetic Acceptance",
        "GIT_COMMITTER_EMAIL": "cedar-synthetic@example.invalid",
        "GIT_AUTHOR_DATE": "2000-01-01T00:00:00+00:00",
        "GIT_COMMITTER_DATE": "2000-01-01T00:00:00+00:00",
    })
    return result


class Agent:
    """One request at a time; bounded frames, waits, queues and pipe readers."""
    def __init__(self, binary, root, *, trusted=True, env=None):
        args = [str(binary), "--root", str(root)]
        if trusted:
            args.append("--allow-run")
        self.process = subprocess.Popen(
            args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, env=env or clean_environment(),
            start_new_session=(os.name != "nt"))
        self.frames = queue.Queue(maxsize=4)
        self.counter = 0
        self.pending = None
        self.closed = False
        self.reader_failure = None
        self.stderr_bytes = 0
        self.readers = [threading.Thread(target=self._stdout, daemon=True),
                        threading.Thread(target=self._stderr, daemon=True)]
        for reader in self.readers:
            reader.start()

    def _stdout(self):
        try:
            while True:
                raw = self.process.stdout.readline(MAX_FRAME + 1)
                if not raw:
                    return
                if len(raw) > MAX_FRAME or not raw.endswith(b"\n"):
                    self.reader_failure = "agent_frame_limit"
                    return
                try:
                    value = json.loads(raw.decode("utf-8", errors="strict"))
                    self.frames.put(value, timeout=1)
                except (ValueError, UnicodeError, queue.Full):
                    self.reader_failure = "agent_frame_invalid"
                    return
        except (OSError, ValueError):
            self.reader_failure = "agent_reader_failed"

    def _stderr(self):
        try:
            while True:
                chunk = self.process.stderr.read(8192)
                if not chunk:
                    return
                self.stderr_bytes = min(MAX_FRAME, self.stderr_bytes + len(chunk))
        except (OSError, ValueError):
            self.reader_failure = "agent_reader_failed"

    def send(self, operation, **fields):
        require(self.pending is None, "agent_request_overlap")
        self.counter += 1
        self.pending = self.counter
        frame = json.dumps({"id": self.counter,
                            "op": {"type": operation, **fields}}).encode() + b"\n"
        # All our requests fit one small pipe write, including Windows paths.
        require(len(frame) < 4096, "agent_request_limit")
        self.process.stdin.write(frame)
        self.process.stdin.flush()

    def receive(self, *, error=None, timeout=REQUEST_SECONDS):
        deadline = time.monotonic() + timeout
        while True:
            if self.reader_failure:
                raise Failure(self.reader_failure)
            remaining = deadline - time.monotonic()
            require(remaining > 0, "agent_response_deadline")
            try:
                response = self.frames.get(timeout=min(remaining, .05))
                break
            except queue.Empty:
                require(self.process.poll() is None, "agent_unexpected_exit")
        require(type(response) is dict and response.get("id") == self.pending,
                "agent_response_identity")
        self.pending = None
        result = response.get("result")
        require(type(result) is dict, "agent_result_invalid")
        if error is None:
            require(set(result) == {"Ok"}, "agent_unexpected_error")
            require(type(result["Ok"]) is dict, "agent_payload_invalid")
            return result["Ok"]
        require(set(result) == {"Err"} and result["Err"].get("code") == error,
                "agent_error_mismatch")
        return result["Err"]

    def call(self, operation, *, error=None, timeout=REQUEST_SECONDS, **fields):
        self.send(operation, **fields)
        return self.receive(error=error, timeout=timeout)

    def close(self, *, killed=False):
        if self.closed:
            return
        self.closed = True
        self.process.stdin.close()
        forced = False
        try:
            self.process.wait(timeout=REQUEST_SECONDS if self.pending is not None else 5)
        except subprocess.TimeoutExpired:
            forced = True
            # Popen retains this exact, unreaped process identity.
            self.process.kill()
            self.process.wait(timeout=5)
        for reader in self.readers:
            reader.join(timeout=2)
        require(not any(reader.is_alive() for reader in self.readers), "agent_reader_leak")
        self.process.stdout.close()
        self.process.stderr.close()
        require(not forced, "agent_shutdown_deadline")
        require(killed or self.process.returncode == 0, "agent_shutdown_failed")

    def __enter__(self):
        return self

    def __exit__(self, kind, value, trace):
        self.close()


class RetainedProcess:
    """Pin a ready generated helper by pidfd / Windows handle, never PID polling.

    The helper cannot finish before its private release file is created. Its
    readiness token and executable are verified while acquiring the handle.
    Cleanup signals only this retained identity, never a name or reused PID.
    """
    def __init__(self, ready, token):
        deadline = time.monotonic() + 5
        while not ready.exists() and time.monotonic() < deadline:
            time.sleep(.01)
        require(ready.is_file() and ready.stat().st_size < 2048, "helper_not_ready")
        value = json.loads(ready.read_text(encoding="utf-8"))
        require(value.get("token") == token and type(value.get("pid")) is int,
                "helper_readiness_invalid")
        pid = value["pid"]
        require(pid > 0, "helper_identity_invalid")
        self.native = None
        if os.name == "nt":
            from ctypes import wintypes as w
            self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            signatures = {
                "OpenProcess": ([w.DWORD, w.BOOL, w.DWORD], w.HANDLE),
                "CloseHandle": ([w.HANDLE], w.BOOL),
                "WaitForSingleObject": ([w.HANDLE, w.DWORD], w.DWORD),
                "TerminateProcess": ([w.HANDLE, w.UINT], w.BOOL),
                "GetProcessTimes": ([w.HANDLE] + [ctypes.POINTER(w.FILETIME)] * 4, w.BOOL),
                "QueryFullProcessImageNameW": ([w.HANDLE, w.DWORD, w.LPWSTR,
                                                ctypes.POINTER(w.DWORD)], w.BOOL),
            }
            for name, (args, result) in signatures.items():
                function = getattr(self.kernel, name)
                function.argtypes, function.restype = args, result
            handle = self.kernel.OpenProcess(0x100000 | 0x1000 | 0x0001, False, pid)
            require(bool(handle), "helper_handle_unavailable")
            self.native = handle
            try:
                text = ctypes.create_unicode_buffer(32768)
                length = w.DWORD(len(text))
                require(self.kernel.QueryFullProcessImageNameW(handle, 0, text,
                                                               ctypes.byref(length)),
                        "helper_image_unavailable")
                require(os.path.samefile(text.value, sys.executable),
                        "helper_image_mismatch")
                times = [w.FILETIME() for _ in range(4)]
                require(self.kernel.GetProcessTimes(handle, *(ctypes.byref(item) for item in times)),
                        "helper_creation_unavailable")
                created = (times[0].dwHighDateTime << 32) | times[0].dwLowDateTime
                require(created == value.get("created"), "helper_creation_mismatch")
                require(self.alive(), "helper_exited_before_retention")
            except BaseException:
                self.kernel.CloseHandle(handle)
                self.native = None
                raise
        else:
            require(hasattr(os, "pidfd_open") and hasattr(signal, "pidfd_send_signal"),
                    "pidfd_required")
            # The gated helper cannot exit between this check and pidfd_open.
            proc = Path("/proc", str(pid))
            require(os.path.realpath(proc / "exe") == os.path.realpath(sys.executable),
                    "helper_image_mismatch")
            require(token.encode() in (proc / "cmdline").read_bytes().split(b"\0"),
                    "helper_command_identity")
            self.native = os.pidfd_open(pid)
            try:
                current = (proc / "stat").read_text()
                created = int(current[current.rfind(")") + 2:].split()[19])
                require(created == value.get("created"), "helper_creation_mismatch")
                require(token.encode() in (proc / "cmdline").read_bytes().split(b"\0"),
                        "helper_command_identity")
                require(self.alive(), "helper_exited_before_retention")
            except BaseException:
                os.close(self.native)
                self.native = None
                raise

    def alive(self):
        if os.name == "nt":
            result = self.kernel.WaitForSingleObject(self.native, 0)
            require(result in (0, 258), "helper_wait_failed")
            return result == 258
        return not select.select([self.native], [], [], 0)[0]

    def wait_dead(self, seconds=3):
        deadline = time.monotonic() + seconds
        while self.alive() and time.monotonic() < deadline:
            time.sleep(.01)
        require(not self.alive(), "helper_survived_owner_cleanup")

    def close(self):
        if self.native is None:
            return
        try:
            if self.alive():
                if os.name == "nt":
                    require(self.kernel.TerminateProcess(self.native, 125),
                            "helper_emergency_cleanup_failed")
                else:
                    signal.pidfd_send_signal(self.native, signal.SIGKILL)
                self.wait_dead()
        finally:
            if os.name == "nt":
                self.kernel.CloseHandle(self.native)
            else:
                os.close(self.native)
            self.native = None


# The known helper is an intentional execution-trust boundary, not a fake Git.
# Gates establish readiness and let us retain its exact identity before any
# timeout/flood/root-exit test. Each helper also has a finite fallback lifetime.
HELPER = r'''import ctypes, json, os, pathlib, subprocess, sys, time
mode, base, token = sys.argv[1:]
base = pathlib.Path(base)
ready = base / (token + ".ready")
release = base / (token + ".release")
if mode == "root_exit":
    data = sys.stdin.buffer.read()
    child = subprocess.Popen([sys.executable, "-I", "-S", __file__, "descendant", str(base), token],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
else:
    created = None
    if os.name == "nt":
        from ctypes import wintypes as w
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetCurrentProcess.restype = w.HANDLE
        kernel.GetProcessTimes.argtypes = [w.HANDLE] + [ctypes.POINTER(w.FILETIME)] * 4
        kernel.GetProcessTimes.restype = w.BOOL
        values = [w.FILETIME() for _ in range(4)]
        if not kernel.GetProcessTimes(kernel.GetCurrentProcess(), *(ctypes.byref(v) for v in values)):
            sys.exit(121)
        created = (values[0].dwHighDateTime << 32) | values[0].dwLowDateTime
    else:
        current = pathlib.Path("/proc/self/stat").read_text()
        created = int(current[current.rfind(")") + 2:].split()[19])
    temporary = ready.with_suffix(".writing")
    temporary.write_text(json.dumps({"pid": os.getpid(), "token": token, "created": created}), encoding="utf-8")
    temporary.replace(ready)
deadline = time.monotonic() + 90
while not release.exists() and time.monotonic() < deadline:
    time.sleep(.01)
if mode == "root_exit":
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()
elif mode == "task":
    pass
elif mode == "flood":
    while time.monotonic() < deadline:
        os.write(2, b"synthetic bounded filter output\n" * 1024)
else:
    while time.monotonic() < deadline:
        time.sleep(.05)
'''


class Fixture:
    def __init__(self, root, git):
        self.root, self.git = root, git
        root.mkdir()
        self.command("init", "--initial-branch=synthetic", "--template=")
        for key, value in (("user.name", "Cedar Synthetic Acceptance"),
                           ("user.email", "cedar-synthetic@example.invalid"),
                           ("commit.gpgSign", "false"), ("core.autocrlf", "false"),
                           ("core.fileMode", "false"), ("core.quotePath", "true")):
            self.command("config", "--local", key, value)

    def command(self, *args):
        require(args[0] in ("init", "config", "add", "commit"), "setup_operation_forbidden")
        result = subprocess.run([str(self.git), "--no-pager", "--literal-pathspecs",
                                 "-c", "core.hooksPath=" + os.devnull,
                                 "-c", "core.fsmonitor=false", *args],
                                cwd=self.root, env=clean_environment(),
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=20)
        require(result.returncode == 0, "fixture_git_failed")
        require(len(result.stdout) + len(result.stderr) < 256 * 1024, "fixture_output_limit")

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content.encode("utf-8") if isinstance(content, str) else content)

    def add(self, *names):
        self.command("add", "--", *names)

    def commit(self):
        self.command("commit", "--quiet", "--no-gpg-sign", "-m", "Synthetic acceptance baseline")

    def snapshot(self):
        # Includes index, HEAD, all refs/config/objects/logs and every worktree
        # entry. Atime is deliberately excluded because reads may update it.
        entries = {}
        for directory, dirs, files in os.walk(self.root, followlinks=False):
            for name in sorted(dirs + files):
                path = Path(directory, name)
                metadata = path.lstat()
                relative = path.relative_to(self.root).as_posix()
                if stat.S_ISLNK(metadata.st_mode):
                    content = os.fsencode(os.readlink(path))
                elif stat.S_ISREG(metadata.st_mode):
                    content = path.read_bytes()
                else:
                    content = b""
                entries[relative] = (metadata.st_mode, metadata.st_mtime_ns,
                                     hashlib.sha256(content).digest())
        return entries


def hello(agent, root):
    result = agent.call("hello")
    require(result.get("type") == "hello" and result.get("protocol") == 4,
            "hello_protocol")
    # Rust canonicalizes Windows roots to a verbatim drive spelling; compare
    # the real directory identity rather than that presentation prefix.
    require(os.path.samefile(result["root"], root), "hello_root")
    capabilities = result.get("agent", {}).get("capabilities", [])
    require({"git_changes", "git_diff", "run_start", "run_poll", "run_cancel"}
            <= set(capabilities), "hello_capabilities")


def changes(agent, git):
    result = agent.call("git_changes", git_executable=str(git))
    require(result.get("type") == "git_changes" and type(result.get("entries")) is list,
            "changes_payload")
    entries = result["entries"]
    require(len({entry["path"] for entry in entries}) == len(entries), "changes_duplicate_path")
    for entry in entries:
        require(set(entry) == {"path", "index", "worktree", "kind", "can_diff_staged",
                               "can_diff_unstaged"}, "changes_schema")
        require(type(entry["path"]) is str and len(entry["index"]) == 1 and
                len(entry["worktree"]) == 1 and
                type(entry["can_diff_staged"]) is bool and
                type(entry["can_diff_unstaged"]) is bool, "changes_field_types")
    return {entry["path"]: entry for entry in entries}


def diff(agent, git, path, kind):
    result = agent.call("git_diff", git_executable=str(git), path=path, kind=kind)
    require(result.get("type") == "git_diff" and result.get("path") == path and
            result.get("kind") == kind and type(result.get("text")) is str,
            "diff_payload_identity")
    return result["text"]


def readback(agent, fixture, path):
    data = (fixture.root / path).read_bytes()
    result = agent.call("read", path=path)
    require(result.get("path") == path and result.get("text") == data.decode("utf-8") and
            result.get("revision") == hashlib.sha256(data).hexdigest(), "source_readback")


def check_normal(binary, git, base):
    global STAGE
    STAGE = "normal_views"
    fixture = Fixture(base / "repository Unicode 你好", git)
    names = ["plain.txt", "double.txt", "staged.txt", "gone parent/deleted.txt",
             "staged-delete.txt", "rename-old.txt", "目录/你好 space.txt", "-leading.txt",
             "literal[abc].txt", "literala.txt"]
    if os.name != "nt":
        names.extend(["tab\tname.txt", "line\nname.txt", "literal*.txt"])
    for index, name in enumerate(names):
        fixture.write(name, "BASE_%d\n" % index)
    fixture.write("binary.bin", b"base\0binary\n")
    fixture.write(".gitignore", "ignored/\n*.ignored\n")
    fixture.add(".")
    fixture.commit()
    baseline = fixture.snapshot()
    with Agent(binary, fixture.root, trusted=False) as agent:
        hello(agent, fixture.root)
        agent.call("git_changes", git_executable="not-an-absolute-program", error="run_disabled")
        agent.call("git_diff", git_executable="not-an-absolute-program", path="../outside",
                   kind="unstaged", error="run_disabled")
    with Agent(binary, fixture.root) as agent:
        hello(agent, fixture.root)
        require(changes(agent, git) == {}, "clean_status")
    require(fixture.snapshot() == baseline, "clean_view_mutation")

    for index, name in enumerate(names):
        fixture.write(name, "CHANGED_%d\n" % index)
    fixture.add("double.txt", "staged.txt")
    fixture.write("double.txt", "WORKTREE_DOUBLE\n")
    (fixture.root / "gone parent/deleted.txt").unlink()
    (fixture.root / "gone parent").rmdir()
    (fixture.root / "staged-delete.txt").unlink()
    fixture.add("staged-delete.txt")
    (fixture.root / "rename-old.txt").rename(fixture.root / "rename-new.txt")
    fixture.add("rename-old.txt", "rename-new.txt")
    fixture.write("binary.bin", b"changed\0binary\n")
    fixture.write("untracked nested/new space.txt", "UNTRACKED\n")
    fixture.write("ignored/hidden.txt", "IGNORED\n")
    expected = set(names) - {"rename-old.txt"}
    expected |= {"rename-old.txt", "rename-new.txt", "binary.bin", "untracked nested/new space.txt"}
    baseline = fixture.snapshot()
    with Agent(binary, fixture.root) as agent:
        hello(agent, fixture.root)
        entries = changes(agent, git)
        require(set(entries) == expected, "status_exact_paths")
        pairs = {"plain.txt": (".", "M"), "double.txt": ("M", "M"),
                 "staged.txt": ("M", "."), "gone parent/deleted.txt": (".", "D"),
                 "staged-delete.txt": ("D", "."), "rename-old.txt": ("D", "."),
                 "rename-new.txt": ("A", ".")}
        for path, pair in pairs.items():
            entry = entries[path]
            require((entry["index"], entry["worktree"]) == pair and entry["kind"] == "file",
                    "status_typed_change")
            require(entry["can_diff_staged"] == (pair[0] != ".") and
                    entry["can_diff_unstaged"] == (pair[1] != "."), "status_diff_eligibility")
        staged = diff(agent, git, "double.txt", "staged")
        unstaged = diff(agent, git, "double.txt", "unstaged")
        require("-BASE_1\n" in staged and "+CHANGED_1\n" in staged and
                "WORKTREE_DOUBLE" not in staged, "staged_index_scope")
        require("-CHANGED_1\n" in unstaged and "+WORKTREE_DOUBLE\n" in unstaged and
                "BASE_1" not in unstaged, "unstaged_disk_scope")
        for name in names:
            if name in pairs and name != "plain.txt":
                continue
            text = diff(agent, git, name, "unstaged")
            index = names.index(name)
            require("+CHANGED_%d\n" % index in text and text.count("diff --git ") == 1,
                    "literal_selected_diff")
            readback(agent, fixture, name)
        require("deleted file mode" in diff(agent, git, "gone parent/deleted.txt", "unstaged"),
                "deleted_parent_diff")
        require("deleted file mode" in diff(agent, git, "staged-delete.txt", "staged"),
                "staged_delete_diff")
        require("Binary files " in diff(agent, git, "binary.bin", "unstaged"), "binary_summary")
        untracked = entries["untracked nested/new space.txt"]
        require(untracked["kind"] == "untracked" and not untracked["can_diff_staged"] and
                not untracked["can_diff_unstaged"], "untracked_status_only")
        agent.call("git_diff", git_executable=str(git), path=untracked["path"], kind="unstaged",
                   error="git_diff_unavailable")
        readback(agent, fixture, untracked["path"])
        invalid = ["../outside", ".git/config", "", "./plain.txt", "目录", str(base / "outside")]
        if os.name == "nt":
            invalid.extend(["NUL", "CON.txt", "COM¹.txt", "LPT²", "bad?.txt", "bad*.txt",
                            "CON .txt", "NUL .txt", "COM1 .txt", "bad|name.txt",
                            "trailing. ", "plain.txt:alternate"])
        for path in invalid:
            agent.call("git_diff", git_executable=str(git), path=path, kind="unstaged",
                       error="invalid_path")
    require(fixture.snapshot() == baseline, "normal_view_mutation")

    STAGE = "hostile_environment"
    decoy = Fixture(base / "decoy repository", git)
    decoy.write("wrong.txt", "WRONG_REPOSITORY\n")
    hostile = clean_environment()
    marker_keys = ("GIT_TRACE", "GIT_TRACE_SETUP", "GIT_TRACE2_EVENT", "GIT_TRACE2_PERF",
                   "GIT_REDIRECT_STDOUT", "GIT_REDIRECT_STDERR")
    markers = [base / ("forbidden_marker_%d" % index) for index in range(len(marker_keys))]
    hostile.update(dict(zip(marker_keys, map(str, markers))))
    hostile.update({"GIT_DIR": str(decoy.root / ".git"), "GIT_WORK_TREE": str(decoy.root),
                    "GIT_COMMON_DIR": str(decoy.root / ".git"),
                    "GIT_INDEX_FILE": str(base / "forbidden_index"),
                    "GIT_OBJECT_DIRECTORY": str(decoy.root / ".git/objects"),
                    "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "core.bare",
                    "GIT_CONFIG_VALUE_0": "true", "GIT_CONFIG_PARAMETERS": "'core.bare=true'",
                    "GIT_EXTERNAL_DIFF": "cedar-deliberately-missing-diff",
                    "GIT_NAMESPACE": "unrelated", "GIT_LITERAL_PATHSPECS": "0"})
    if os.name == "nt":
        # Windows ordinal case folding also equates dotless U+0131 with I.
        # Avoid duplicate aliases in the incoming block; test the real lookup.
        hostile["GıT_INDEX_FILE"] = hostile.pop("GIT_INDEX_FILE")
        hostile["GıT_TRACE"] = hostile.pop("GIT_TRACE")
    decoy_before = decoy.snapshot()
    with Agent(binary, fixture.root, env=hostile) as agent:
        hello(agent, fixture.root)
        require(changes(agent, git) == entries, "hostile_status_isolation")
        require(diff(agent, git, "double.txt", "staged") == staged, "hostile_diff_isolation")
    require(not any(path.exists() for path in markers) and
            not (base / "forbidden_index").exists(), "hostile_marker_created")
    require(fixture.snapshot() == baseline and decoy.snapshot() == decoy_before,
            "hostile_view_mutation")
    return len(entries)


def check_unborn_and_bounds(binary, git, base):
    global STAGE
    STAGE = "unborn_and_bounds"
    fixture = Fixture(base / "unborn repository", git)
    fixture.write("new 你好.txt", "UNBORN_STAGED\n")
    fixture.add("new 你好.txt")
    baseline = fixture.snapshot()
    with Agent(binary, fixture.root) as agent:
        entries = changes(agent, git)
        require(entries["new 你好.txt"]["can_diff_staged"], "unborn_eligibility")
        require("+UNBORN_STAGED\n" in diff(agent, git, "new 你好.txt", "staged"), "unborn_diff")
    require(fixture.snapshot() == baseline, "unborn_view_mutation")
    fixture.commit()
    fixture.write("new 你好.txt", "bounded synthetic line\n" * 20000)
    baseline = fixture.snapshot()
    with Agent(binary, fixture.root) as agent:
        agent.call("git_diff", git_executable=str(git), path="new 你好.txt", kind="unstaged",
                   error="output_limit")
        hello(agent, fixture.root)
    require(fixture.snapshot() == baseline, "limited_diff_mutation")
    fixture.write("new 你好.txt", b"invalid UTF-8 text: \xff\n")
    baseline = fixture.snapshot()
    with Agent(binary, fixture.root) as agent:
        agent.call("git_diff", git_executable=str(git), path="new 你好.txt", kind="unstaged",
                   error="invalid_utf8")
    require(fixture.snapshot() == baseline, "invalid_text_diff_mutation")
    if os.name != "nt":
        raw_path = os.fsencode(fixture.root) + b"/invalid-utf8-\xff"
        descriptor = os.open(raw_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        os.close(descriptor)
        try:
            with Agent(binary, fixture.root) as agent:
                agent.call("git_changes", git_executable=str(git), error="invalid_utf8")
        finally:
            os.unlink(raw_path)

    invalid_root = base / "gitfile repository"
    invalid_root.mkdir()
    (invalid_root / ".git").write_text("gitdir: " + str(fixture.root / ".git") + "\n",
                                       encoding="utf-8")
    with Agent(binary, invalid_root) as agent:
        agent.call("git_changes", git_executable=str(git), error="git_repository_unsupported")


def check_missing_promisor_object(binary, git, base):
    """A missing promised blob must fail without fetching or writing objects.

    The only configured remote is another newly generated local repository.
    Its reachable blob has exactly the missing object's identity. This tests
    the combined no-lazy-fetch/disabled-transport boundary; it does not loosen
    either protection or contact any external host to test them independently.
    """
    global STAGE
    STAGE = "missing_promisor_object"
    name = "promisor missing 你好.txt"
    original = b"PROMISOR_BASE\n"
    fixture = Fixture(base / "promisor repository", git)
    backing = Fixture(base / "local backing repository", git)
    for repository in (fixture, backing):
        repository.write(name, original)
        repository.add(name)
        repository.commit()

    # Derive only this generated blob's identity; accept SHA-1 or SHA-256 repos.
    # Check its bounded loose-object contents before removing the owned copy.
    content = b"blob " + str(len(original)).encode("ascii") + b"\0" + original
    identities = [hashlib.sha1(content).hexdigest(), hashlib.sha256(content).hexdigest()]
    candidates = [(identity, fixture.root / ".git/objects" / identity[:2] / identity[2:])
                  for identity in identities]
    candidates = [(identity, path) for identity, path in candidates if path.is_file()]
    require(len(candidates) == 1, "promisor_blob_identity")
    identity, missing = candidates[0]
    source = backing.root / ".git/objects" / identity[:2] / identity[2:]
    for path in (missing, source):
        require(path.is_file() and path.stat().st_size < 4096, "promisor_blob_source")
        decoder = zlib.decompressobj()
        require(decoder.decompress(path.read_bytes(), 4096) == content and decoder.eof and
                not decoder.unused_data, "promisor_blob_contents")

    # Configure a real partial clone relationship entirely through local config;
    # never clone/fetch or use an external URL. The backing commit keeps the blob
    # reachable, so an accidental permitted lazy fetch could restore it.
    for key, value in (("core.repositoryFormatVersion", "1"),
                       ("extensions.partialClone", "origin"),
                       ("remote.origin.url", backing.root.as_uri()),
                       ("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"),
                       ("remote.origin.promisor", "true"),
                       ("remote.origin.partialCloneFilter", "blob:none")):
        fixture.command("config", "--local", key, value)
    fixture.write(name, b"PROMISOR_EDIT\n")
    missing.unlink()
    require(not missing.exists(), "promisor_blob_not_removed")
    before, backing_before = fixture.snapshot(), backing.snapshot()
    with Agent(binary, fixture.root) as agent:
        entries = changes(agent, git)
        require(set(entries) == {name} and entries[name]["kind"] == "file" and
                entries[name]["can_diff_unstaged"], "promisor_status_eligible")
        agent.call("git_diff", git_executable=str(git), path=name, kind="unstaged",
                   error="git_error")
        hello(agent, fixture.root)
        readback(agent, fixture, name)
    require(not missing.exists(), "promisor_blob_restored")
    # Whole-tree snapshots also catch new packfiles, refs, lockfiles or metadata
    # writes even if Git restored an object in a different on-disk representation.
    require(fixture.snapshot() == before, "promisor_repository_mutation")
    require(backing.snapshot() == backing_before, "promisor_backing_mutation")


def check_filter_ownership(binary, git, base):
    global STAGE
    STAGE = "filter_ownership"
    control = base / "owned filter control"
    control.mkdir()
    script = control / "generated_filter.py"
    script.write_text(HELPER, encoding="utf-8")
    fixture = Fixture(base / "trusted filter repository", git)
    fixture.write("filtered.txt", "BASE_FILTER\n")
    fixture.write(".gitattributes", "filtered.txt filter=cedar_acceptance\n")
    fixture.add(".")
    fixture.commit()
    # Equal byte lengths force status to hash content and invoke the filter.
    fixture.write("filtered.txt", "NEXT_FILTER\n")
    agent = Agent(binary, fixture.root)
    task_owner = None
    try:
        hello(agent, fixture.root)
        task_token = uuid.uuid4().hex
        task = agent.call("run_start", program=str(Path(sys.executable).resolve()),
                          args=["-I", "-S", str(script), "task", str(control), task_token],
                          timeout_secs=120)["snapshot"]
        task_owner = RetainedProcess(control / (task_token + ".ready"), task_token)
        for mode, error in (("stall", "command_timeout"), ("flood", "output_limit"),
                            ("root_exit", None)):
            STAGE = "filter_" + mode
            token = uuid.uuid4().hex
            arguments = [str(Path(sys.executable).resolve()), "-I", "-S", str(script),
                         mode, str(control), token]
            # Git executes a clean-filter command through its own shell, including
            # Git for Windows' sh. Quote each owned path using that shell grammar.
            command = " ".join(shlex.quote(value.replace("\\", "/") if os.name == "nt" else value)
                               for value in arguments)
            fixture.command("config", "--local", "filter.cedar_acceptance.clean", command)
            fixture.command("config", "--local", "filter.cedar_acceptance.required", "true")
            baseline = fixture.snapshot()
            agent.send("git_changes", git_executable=str(git))
            owner = RetainedProcess(control / (token + ".ready"), token)
            try:
                (control / (token + ".release")).write_bytes(b"go")
                response = agent.receive(error=error)
                if error is None:
                    require(response.get("type") == "git_changes" and
                            any(entry["path"] == "filtered.txt" for entry in response["entries"]),
                            "filter_root_exit_status")
                owner.wait_dead()
            finally:
                owner.close()
            require(fixture.snapshot() == baseline, "filter_view_mutation")
            require(task_owner.alive(), "independent_task_terminated")
            snapshot = agent.call("run_poll", task_id=task["id"])["snapshot"]
            require(snapshot.get("state") == "running", "independent_task_not_running")
        STAGE = "independent_task_cleanup"
        agent.call("run_cancel", task_id=task["id"])
        deadline = time.monotonic() + 5
        while True:
            snapshot = agent.call("run_poll", task_id=task["id"])["snapshot"]
            if snapshot.get("state") == "cancelled":
                break
            require(time.monotonic() < deadline, "task_cancel_deadline")
            time.sleep(.02)
        task_owner.wait_dead()
    finally:
        if task_owner is not None:
            task_owner.close()
        agent.close()

    # Windows jobs close with an abruptly killed agent. Unix process groups do
    # not provide parent-death containment; this script makes no such claim.
    if os.name == "nt":
        STAGE = "forced_agent_death"
        token = uuid.uuid4().hex
        arguments = [str(Path(sys.executable).resolve()), "-I", "-S", str(script),
                     "stall", str(control), token]
        fixture.command("config", "--local", "filter.cedar_acceptance.clean",
                        " ".join(shlex.quote(value.replace("\\", "/")) for value in arguments))
        agent = Agent(binary, fixture.root)
        owner = None
        try:
            agent.send("git_changes", git_executable=str(git))
            owner = RetainedProcess(control / (token + ".ready"), token)
            # Retained Popen handle targets only this owned normal agent.
            agent.process.kill()
            agent.process.wait(timeout=5)
            owner.wait_dead()
        finally:
            if owner is not None:
                owner.close()
            agent.close(killed=True)


def main():
    global STAGE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent", required=True)
    parser.add_argument("--git", required=True)
    args = parser.parse_args()
    require(os.name == "nt" or sys.platform.startswith("linux"), "unsupported_test_platform")
    agent, git = Path(args.agent), Path(args.git)
    require(agent.is_absolute() and agent.is_file(), "explicit_agent_required")
    require(git.is_absolute() and git.is_file(), "explicit_git_required")
    if os.name == "nt":
        require(agent.suffix.lower() == ".exe" and git.suffix.lower() == ".exe", "native_exe_required")
    STAGE = "git_version"
    version = subprocess.run([str(git), "--no-lazy-fetch", "--version"],
                             env=clean_environment(), stdin=subprocess.DEVNULL,
                             capture_output=True, timeout=10)
    match = re.fullmatch(rb"git version (\d+)\.(\d+)\.(\d+)[^\r\n]*\r?\n?", version.stdout)
    require(version.returncode == 0 and match is not None, "git_version_unavailable")
    require(tuple(int(match.group(i)) for i in (1, 2)) >= (2, 45), "git_2_45_required")
    with tempfile.TemporaryDirectory(prefix="cedar-git-acceptance-") as temporary:
        base = Path(temporary).resolve()
        count = check_normal(agent, git, base)
        check_unborn_and_bounds(agent, git, base)
        check_missing_promisor_object(agent, git, base)
        check_filter_ownership(agent, git, base)
    STAGE = "complete"
    print(json.dumps({"result": "PASS", "suite": "normal_agent_real_git_views",
                      "platform": "windows" if os.name == "nt" else "linux",
                      "assertions": ASSERTIONS, "typed_entries": count,
                      "trust_gate": True, "source_readback": True, "repository_unchanged": True,
                      "hostile_environment_ignored": True, "deadline_cleanup": True,
                      "missing_promisor_failure": True, "promisor_object_absent": True,
                      "output_cleanup": True, "root_exit_cleanup": True,
                      "independent_task_preserved": True, "forced_agent_death": os.name == "nt",
                      "non_utf8_diff_rejected": True,
                      "non_utf8_paths_rejected": os.name != "nt"}, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except BaseException as error:
        if isinstance(error, SystemExit):
            raise
        code = str(error) if isinstance(error, Failure) else "acceptance_failed"
        print(json.dumps({"result": "FAIL", "suite": "normal_agent_real_git_views",
                          "stage": STAGE, "code": code, "assertions": ASSERTIONS}, sort_keys=True))
        sys.exit(1)
