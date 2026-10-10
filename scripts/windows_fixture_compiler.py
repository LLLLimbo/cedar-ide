#!/usr/bin/env python3
"""Select the already-installed CI stable compiler without changing PATH.

Run after the existing Rust toolchain action, using Python 3.12+ on Windows.
Only four read-only commands are allowed. Their combined stdout/stderr is
bounded during capture, never published, and never written to a log. The
selected compiler is exported only after installed-toolchain, file-identity,
and version-agreement checks. No installation or rustup settings are changed.
"""
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time


TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
HOST = "x86_64-pc-windows-msvc"
CALL_SECONDS = 15
TOTAL_SECONDS = 60
REAP_SECONDS = 2
MAX_OUTPUT_BYTES = 64 * 1024
MAX_PATH_CHARS = 4096
REPARSE_POINT = 0x400
ERROR_CODES = {
    "native_windows_required", "python_312_required",
    "invalid_path", "invalid_file", "invalid_directory", "reparse_path",
    "file_identity_changed", "environment_identity_changed", "environment_alias", "environment_write_failed",
    "environment_write_unverified", "installed_toolchain_missing", "invalid_toolchain_list",
    "invalid_compiler_path", "compiler_is_proxy", "invalid_version", "compiler_mismatch",
    "total_timeout", "subprocess_timeout", "subprocess_output_limit", "subprocess_nonzero",
    "output_pipe_missing", "child_cleanup_unverified", "preflight_error",
}


class PreflightError(ValueError):
    """Only fixed categories are eligible for publication."""


def require(condition, code):
    if not condition:
        raise PreflightError(code)


def safe_text(value):
    return (isinstance(value, str) and 0 < len(value) <= MAX_PATH_CHARS
            and not any(ord(char) < 32 or ord(char) == 127
                        or char in "\x85\u2028\u2029" for char in value))


def absolute_path(value):
    require(safe_text(value), "invalid_path")
    if os.name == "nt":
        require(local_windows_path(value), "invalid_path")
    path = Path(value)
    require(path.is_absolute(), "invalid_path")
    return path


def local_windows_path(value):
    """Admit drive-absolute paths, including local extended paths, before I/O."""
    if not safe_text(value):
        return False
    if value.startswith("\\\\?\\"):
        value = value[4:]
    return re.match(r"^[A-Za-z]:[\\/]", value) is not None and ":" not in value[2:]


def signature(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_size,
            info.st_mtime_ns, info.st_ctime_ns)


def regular_info(path):
    # Check the whole route before resolving: resolving first would hide a
    # symlink or Windows junction in an ancestor directory.
    for ancestor in reversed(path.parents):
        info = ancestor.lstat()
        require(not stat.S_ISLNK(info.st_mode)
                and not getattr(info, "st_file_attributes", 0) & REPARSE_POINT,
                "reparse_path")
        require(stat.S_ISDIR(info.st_mode), "invalid_directory")
    info = path.lstat()
    require(not stat.S_ISLNK(info.st_mode)
            and not getattr(info, "st_file_attributes", 0) & REPARSE_POINT,
            "reparse_path")
    require(stat.S_ISREG(info.st_mode), "invalid_file")
    return info


@dataclass(frozen=True)
class FileIdentity:
    path: Path
    facts: tuple

    @classmethod
    def inspect(cls, value, name=None):
        path = absolute_path(value)
        if name is not None:
            require(path.name.lower() == name, "invalid_compiler_path")
        before = regular_info(path)
        canonical = absolute_path(str(path.resolve(strict=True)))
        require(signature(regular_info(canonical)) == signature(before),
                "file_identity_changed")
        return cls(canonical, signature(before))

    def check(self, code="file_identity_changed"):
        require(signature(regular_info(self.path)) == self.facts, code)

    def check_open(self, stream):
        require(signature(os.fstat(stream.fileno())) == self.facts,
                "environment_identity_changed")
        self.check("environment_identity_changed")


def remaining(deadline):
    duration = deadline - time.monotonic()
    require(duration > 0, "total_timeout")
    return duration


def bounded_process(command, environment, deadline):
    """Capture at most 64 KiB, with one deadline for child execution and EOF.

    Nonblocking anonymous pipes are supported on Windows by Python 3.12.
    Cleanup terminates/reaps the direct child only; this is not a process-tree
    containment claim. Pipe EOF is bounded even if a descendant keeps it open.
    """
    budget = remaining(deadline)
    require(budget > REAP_SECONDS, "total_timeout")
    call_deadline = min(time.monotonic() + CALL_SECONDS, deadline - REAP_SECONDS)
    captured = bytearray()
    process = subprocess.Popen(command, env=environment, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    try:
        require(process.stdout is not None, "output_pipe_missing")
        os.set_blocking(process.stdout.fileno(), False)
        open_pipe = True
        while open_pipe or process.poll() is None:
            require(time.monotonic() < call_deadline, "subprocess_timeout")
            chunk = None
            if open_pipe:
                try:
                    chunk = os.read(process.stdout.fileno(),
                                    min(8192, MAX_OUTPUT_BYTES - len(captured) + 1))
                except BlockingIOError:
                    pass
                if chunk == b"":
                    open_pipe = False
                elif chunk is not None:
                    require(len(chunk) <= MAX_OUTPUT_BYTES - len(captured),
                            "subprocess_output_limit")
                    captured.extend(chunk)
            if chunk is None:
                time.sleep(min(0.01, max(0, call_deadline - time.monotonic())))
        require(time.monotonic() < call_deadline, "subprocess_timeout")
        require(process.returncode == 0, "subprocess_nonzero")
    finally:
        try:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=min(REAP_SECONDS, max(0, deadline - time.monotonic())))
        except (OSError, subprocess.TimeoutExpired):
            raise PreflightError("child_cleanup_unverified") from None
        finally:
            if process.stdout is not None:
                process.stdout.close()
    remaining(deadline)
    return bytes(captured)


def output_lines(data, code):
    require(type(data) is bytes and 0 < len(data) <= MAX_OUTPUT_BYTES, code)
    try:
        text = data.decode("utf-8", errors="strict")
    except UnicodeError:
        raise PreflightError(code) from None
    # Only conventional LF or CRLF framing is accepted. strip()/splitlines()
    # would silently accept control characters or extra blank output.
    text = text.replace("\r\n", "\n")
    if text.endswith("\n"):
        text = text[:-1]
    lines = text.split("\n")
    require(all(safe_text(line) and line == line.strip() for line in lines), code)
    return lines


def verify_installed(data):
    lines = output_lines(data, "invalid_toolchain_list")
    names = []
    for line in lines:
        match = re.fullmatch(r"([A-Za-z0-9_.+-]+)(?: \((?:active|default|active, default|default, active)\))?", line)
        require(match is not None, "invalid_toolchain_list")
        names.append(match.group(1))
    require(len(names) == len(set(names)), "invalid_toolchain_list")
    require(names.count(TOOLCHAIN) == 1, "installed_toolchain_missing")


def compiler_from_output(data, rustup):
    lines = output_lines(data, "invalid_compiler_path")
    require(len(lines) == 1, "invalid_compiler_path")
    compiler = FileIdentity.inspect(lines[0], "rustc.exe")
    require(compiler.path.parent.name.lower() == "bin"
            and compiler.path.parent.parent.name.lower() == TOOLCHAIN,
            "invalid_compiler_path")
    require(compiler.facts[:2] != rustup.facts[:2], "compiler_is_proxy")
    return compiler


def version_fields(data):
    lines = output_lines(data, "invalid_version")
    header = re.fullmatch(r"rustc ([0-9]+\.[0-9]+\.[0-9]+) \(([0-9a-f]{7,40}) ([0-9]{4}-[0-9]{2}-[0-9]{2})\)",
                         lines[0])
    require(header is not None, "invalid_version")
    fields = {}
    for line in lines[1:]:
        match = re.fullmatch(r"([A-Za-z][A-Za-z -]*): (.+)", line)
        require(match is not None and match.group(1) not in fields, "invalid_version")
        fields[match.group(1)] = match.group(2)
    require(set(fields) == {"binary", "commit-hash", "commit-date", "host", "release", "LLVM version"},
            "invalid_version")
    require(fields["binary"] == "rustc" and fields["host"] == HOST
            and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", fields["release"]) is not None
            and re.fullmatch(r"[0-9a-f]{40}", fields["commit-hash"]) is not None
            and re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", fields["commit-date"]) is not None
            and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", fields["LLVM version"]) is not None,
            "invalid_version")
    require(header.group(1) == fields["release"]
            and fields["commit-hash"].startswith(header.group(2))
            and header.group(3) == fields["commit-date"], "invalid_version")
    return tuple(fields[key] for key in ("release", "commit-hash", "host"))


def append_rustc(stream, environment_file, compiler, rustup, deadline):
    environment_file.check_open(stream)
    compiler.check()
    value = str(compiler.path)
    require(safe_text(value), "invalid_compiler_path")
    size = environment_file.facts[3]
    prefix = b""
    if size:
        stream.seek(-1, os.SEEK_END)
        if stream.read(1) != b"\n":
            prefix = b"\n"
    payload = prefix + ("RUSTC=" + value + "\n").encode("utf-8", errors="strict")
    stream.seek(0, os.SEEK_END)
    environment_file.check_open(stream)
    compiler.check()
    rustup.check()
    remaining(deadline)
    try:
        require(stream.write(payload) == len(payload), "environment_write_failed")
        stream.flush()
        after = FileIdentity(environment_file.path, signature(os.fstat(stream.fileno())))
        after.check()
        require(after.facts[:2] == environment_file.facts[:2]
                and after.facts[3] == size + len(payload), "environment_write_failed")
        remaining(deadline)
    except Exception as error:
        try:
            restore_length(stream, environment_file)
        except Exception:
            raise PreflightError("environment_write_unverified") from None
        if isinstance(error, PreflightError) and str(error) == "total_timeout":
            raise
        raise PreflightError("environment_write_failed") from None


def restore_length(stream, before):
    stream.truncate(before.facts[3])
    stream.flush()
    restored = signature(os.fstat(stream.fileno()))
    require(restored[:2] == before.facts[:2] and restored[3] == before.facts[3],
            "environment_write_unverified")
    require(signature(regular_info(before.path)) == restored, "environment_write_unverified")


def run(environment=None):
    require(sys.platform == "win32", "native_windows_required")
    require(sys.version_info >= (3, 12), "python_312_required")
    deadline = time.monotonic() + TOTAL_SECONDS
    source = dict(os.environ if environment is None else environment)
    cargo_home = absolute_path(source.get("CARGO_HOME"))
    rustup = FileIdentity.inspect(str(cargo_home / "bin" / "rustup.exe"), "rustup.exe")
    environment_file = FileIdentity.inspect(source.get("GITHUB_ENV"))
    require(environment_file.facts[:2] != rustup.facts[:2], "environment_alias")
    child_environment = dict(source)
    # Supported rustup switch; listing the installed toolchain is an additional
    # prerequisite, and `run` never receives the optional --install flag.
    # https://rust-lang.github.io/rustup/environment-variables.html
    child_environment["RUSTUP_AUTO_INSTALL"] = "0"
    with environment_file.path.open("r+b", buffering=0) as stream:
        environment_file.check_open(stream)

        def invoke(arguments, compiler=None):
            remaining(deadline)
            rustup.check()
            environment_file.check_open(stream)
            if compiler is not None:
                compiler.check()
            data = bounded_process(arguments, child_environment, deadline)
            rustup.check()
            environment_file.check_open(stream)
            if compiler is not None:
                compiler.check()
            remaining(deadline)
            return data

        verify_installed(invoke([str(rustup.path), "toolchain", "list"]))
        compiler = compiler_from_output(
            invoke([str(rustup.path), "which", "--toolchain", "stable", "rustc"]), rustup)
        require(environment_file.facts[:2] != compiler.facts[:2], "environment_alias")
        direct = version_fields(invoke([str(compiler.path), "-vV"], compiler))
        managed = version_fields(invoke([str(rustup.path), "run", "stable", "rustc", "-vV"], compiler))
        require(direct == managed, "compiler_mismatch")
        remaining(deadline)
        rustup.check()
        append_rustc(stream, environment_file, compiler, rustup, deadline)
    # A late close still fails the step. A completed, valid append need not be
    # reopened or undone: dependent CI steps cannot proceed after this failure.
    remaining(deadline)
    return {"schema_version": 1, "suite": "windows_fixture_compiler", "result": "PASS",
            "code": "stable_compiler_verified", "installed_stable_verified": True,
            "compiler_metadata_agrees": True, "rustc_exported": True, "path_unchanged": True}


def main():
    try:
        receipt = run()
    except Exception as error:
        code = str(error) if isinstance(error, PreflightError) else "preflight_error"
        receipt = {"schema_version": 1, "suite": "windows_fixture_compiler", "result": "FAIL",
                   "code": code if code in ERROR_CODES else "preflight_error"}
    print(json.dumps(receipt, sort_keys=True))
    return 0 if receipt["result"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
