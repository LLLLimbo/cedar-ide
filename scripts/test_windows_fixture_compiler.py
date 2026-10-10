#!/usr/bin/env python3
"""Portable compiler-preflight tests; never launch Rust or install a toolchain."""
from contextlib import contextmanager, redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest import mock

import windows_fixture_compiler as preflight


def version(release="1.90.0", commit="a" * 40, host=preflight.HOST):
    return (f"rustc {release} ({commit[:9]} 2025-09-14)\n"
            f"binary: rustc\ncommit-hash: {commit}\ncommit-date: 2025-09-14\n"
            f"host: {host}\nrelease: {release}\nLLVM version: 20.1.8\n").encode("utf-8")


class SignatureTests(unittest.TestCase):
    @staticmethod
    def info(**changes):
        values = {"st_dev": 7, "st_ino": (1 << 100) + 9, "st_mode": 0o100600,
                  "st_size": 31, "st_mtime_ns": 1200, "st_ctime_ns": 1400,
                  "st_birthtime_ns": 800}
        values.update(changes)
        return SimpleNamespace(**values)

    def test_windows_common_birthtime_ignores_only_incompatible_ctime_meaning(self):
        path = self.info(st_ctime_ns=800)
        descriptor = self.info(st_ctime_ns=1400)
        self.assertEqual(preflight.signature(path, windows=True),
                         preflight.signature(descriptor, windows=True))

    def test_windows_every_comparable_field_remains_exact(self):
        original = preflight.signature(self.info(), windows=True)
        for key, value in {"st_dev": 8, "st_ino": (1 << 101) + 9,
                           "st_mode": 0o100400, "st_size": 32,
                           "st_mtime_ns": 1201, "st_birthtime_ns": 801}.items():
            with self.subTest(field=key):
                self.assertNotEqual(original, preflight.signature(
                    self.info(**{key: value}), windows=True))
        self.assertEqual(original[1], (1 << 100) + 9)

    def test_windows_missing_or_invalid_birthtime_fails_closed(self):
        missing = self.info()
        del missing.st_birthtime_ns
        for info in [missing, self.info(st_birthtime_ns=None),
                     self.info(st_birthtime_ns=True), self.info(st_birthtime_ns=800.0)]:
            with self.subTest(info=info):
                with self.assertRaisesRegex(preflight.PreflightError,
                                            "file_identity_unsupported"):
                    preflight.signature(info, windows=True)

    def test_unix_ctime_is_still_exact_and_birthtime_is_not_substituted(self):
        original = preflight.signature(self.info(), windows=False)
        self.assertNotEqual(original, preflight.signature(
            self.info(st_ctime_ns=1401), windows=False))
        self.assertEqual(original, preflight.signature(
            self.info(st_birthtime_ns=801), windows=False))


class PreflightTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cedar-compiler-test-")
        self.addCleanup(temporary.cleanup)
        # A canonical fixture root is essential on Windows, where the runtime
        # may change drive spelling when resolving an existing path.
        self.root = Path(temporary.name).resolve()
        self.cargo_home = self.root / "cargo"
        self.rustup = self.cargo_home / "bin" / "rustup.exe"
        self.compiler = self.root / "toolchains" / preflight.TOOLCHAIN / "bin" / "rustc.exe"
        for executable in (self.rustup, self.compiler):
            executable.parent.mkdir(parents=True, exist_ok=True)
            executable.write_bytes(b"owned fake executable; never executed")
        self.environment_file = self.root / "github-env"
        self.original = b"EXISTING=value\n"
        # Explicit bytes avoid platform newline translation in the fixture.
        self.environment_file.write_bytes(self.original)
        self.environment = {"CARGO_HOME": str(self.cargo_home),
                            "GITHUB_ENV": str(self.environment_file),
                            "PATH": "unchanged synthetic PATH", "RUSTUP_AUTO_INSTALL": "1"}
        self.calls = []
        self.outputs = [
            (preflight.TOOLCHAIN + " (active, default)\n").encode("utf-8"),
            (str(self.compiler) + "\n").encode("utf-8"), version(), version(),
        ]

    def execute(self, effect=None):
        def child(command, environment, deadline):
            index = len(self.calls)
            self.calls.append((list(command), dict(environment), deadline))
            if effect is not None:
                effect(index)
            return self.outputs[index]

        with mock.patch.object(preflight.sys, "platform", "win32"), \
                mock.patch.object(preflight, "bounded_process", side_effect=child):
            return preflight.run(self.environment)

    def assert_rejected(self, code=None, effect=None):
        with self.assertRaises(preflight.PreflightError) as raised:
            self.execute(effect)
        if code is not None:
            self.assertEqual(str(raised.exception), code)
        self.assertEqual(self.environment_file.read_bytes(), self.original)

    def test_success_exports_the_compiler_after_four_read_only_checks(self):
        before = dict(self.environment)
        process_before = dict(os.environ)
        receipt = self.execute()
        self.assertEqual(receipt["result"], "PASS")
        self.assertTrue(receipt["compiler_metadata_agrees"])
        self.assertEqual(self.environment_file.read_bytes(),
                         self.original + ("RUSTC=" + str(self.compiler.resolve()) + "\n").encode("utf-8"))
        self.assertEqual(self.environment, before)
        self.assertEqual(dict(os.environ), process_before)
        self.assertEqual([call[0] for call in self.calls], [
            [str(self.rustup.resolve()), "toolchain", "list"],
            [str(self.rustup.resolve()), "which", "--toolchain", "stable", "rustc"],
            [str(self.compiler.resolve()), "-vV"],
            [str(self.rustup.resolve()), "run", "stable", "rustc", "-vV"],
        ])
        self.assertEqual(len({call[2] for call in self.calls}), 1)
        for _, environment, _ in self.calls:
            self.assertEqual(environment, {**before, "RUSTUP_AUTO_INSTALL": "0"})
        self.assertNotIn(str(self.root), json.dumps(receipt))

    def test_success_preserves_bytes_and_separates_a_missing_final_newline(self):
        for original in (b"", b"EXISTING=value", b"EXISTING=value\r\n"):
            with self.subTest(original=original):
                self.environment_file.write_bytes(original)
                self.calls.clear()
                self.execute()
                prefix = b"\n" if original and not original.endswith(b"\n") else b""
                self.assertEqual(self.environment_file.read_bytes(),
                                 original + prefix + ("RUSTC=" + str(self.compiler.resolve()) + "\n").encode())

    def test_crlf_tool_output_is_accepted(self):
        self.outputs = [output.replace(b"\n", b"\r\n") for output in self.outputs]
        self.assertEqual(self.execute()["result"], "PASS")

    def test_absent_stable_stops_before_which_or_any_compiler_execution(self):
        self.outputs[0] = b"nightly-x86_64-pc-windows-msvc (default)\n"
        self.assert_rejected("installed_toolchain_missing")
        self.assertEqual(len(self.calls), 1)

    def test_duplicate_and_malformed_toolchain_list_fail_closed(self):
        original = self.outputs[0]
        for output in (original + original, b"", b"\xff", original + b"\n",
                       (preflight.TOOLCHAIN + " (active, active)\n").encode(),
                       (preflight.TOOLCHAIN + " (default, default)\n").encode(),
                       original + b"private warning\n", b"no installed toolchains\n"):
            with self.subTest(output=output):
                self.outputs[0] = output
                self.calls.clear()
                self.assert_rejected("invalid_toolchain_list")
                self.assertEqual(len(self.calls), 1)

    def test_single_absolute_native_path_is_required(self):
        for output in (b"", b"rustc.exe\n", b"relative/rustc.exe\n", b"\xff\n",
                       (str(self.compiler) + "\n" + str(self.compiler) + "\n").encode(),
                       (str(self.compiler) + "\n\n").encode(),
                       (str(self.compiler) + "\0\n").encode(),
                       (str(self.compiler) + "\r\nINJECTED=value\n").encode(),
                       (str(self.compiler) + "\t\n").encode(),
                       (str(self.compiler) + "\u2028\n").encode(),
                       (str(self.compiler.with_suffix(".cmd")) + "\n").encode(),
                       (" " + str(self.compiler) + "\n").encode()):
            with self.subTest(output=output):
                self.outputs[1] = output
                self.calls.clear()
                self.assert_rejected()
                self.assertEqual(len(self.calls), 2)

    def test_existing_executable_outside_stable_directory_is_rejected(self):
        other = self.root / "rustc.exe"
        other.write_bytes(b"other executable")
        self.outputs[1] = (str(other) + "\n").encode()
        self.assert_rejected("invalid_compiler_path")

    def test_rustup_proxy_hardlink_is_rejected(self):
        self.compiler.unlink()
        os.link(self.rustup, self.compiler)
        self.assert_rejected("compiler_is_proxy")

    def test_missing_compiler_or_rustup_never_exports_or_uses_path(self):
        for executable, count in ((self.rustup, 0), (self.compiler, 2)):
            with self.subTest(executable=executable.name):
                original = executable.read_bytes()
                executable.unlink()
                self.calls.clear()
                with self.assertRaises(OSError):
                    self.execute()
                self.assertEqual(len(self.calls), count)
                self.assertEqual(self.environment_file.read_bytes(), self.original)
                executable.write_bytes(original)

    def test_directory_is_not_an_executable(self):
        self.compiler.unlink()
        self.compiler.mkdir()
        self.assert_rejected("invalid_file")

    def test_relative_or_control_character_environment_input_is_rejected(self):
        for key in ("CARGO_HOME", "GITHUB_ENV"):
            for value in (None, "relative", "relative\nINJECT=value", "bad\0path"):
                with self.subTest(key=key, value=value):
                    original = self.environment[key]
                    self.environment[key] = value
                    self.calls.clear()
                    self.assert_rejected("invalid_path")
                    self.assertEqual(self.calls, [])
                    self.environment[key] = original

    def test_environment_file_must_already_exist(self):
        self.environment_file.unlink()
        with self.assertRaises(OSError):
            self.execute()
        self.assertFalse(self.environment_file.exists())
        self.assertEqual(self.calls, [])

    def test_environment_directory_is_rejected(self):
        self.environment_file.unlink()
        self.environment_file.mkdir()
        with self.assertRaisesRegex(preflight.PreflightError, "invalid_file"):
            self.execute()
        self.assertEqual(self.calls, [])
        self.assertEqual(list(self.environment_file.iterdir()), [])

    def test_environment_file_cannot_alias_either_executable(self):
        for executable, expected_calls in ((self.rustup, 0), (self.compiler, 2)):
            with self.subTest(executable=executable.name):
                for hardlink in (False, True):
                    before = executable.read_bytes()
                    self.calls.clear()
                    alias = executable
                    if hardlink:
                        alias = self.root / "aliased-env"
                        os.link(executable, alias)
                    self.environment["GITHUB_ENV"] = str(alias)
                    with self.assertRaisesRegex(preflight.PreflightError, "environment_alias"):
                        self.execute()
                    self.assertEqual(len(self.calls), expected_calls)
                    self.assertEqual(executable.read_bytes(), before)
                    if hardlink:
                        alias.unlink()

    def test_symlink_environment_file_is_rejected_without_touching_target(self):
        target = self.root / "target"
        target.write_bytes(self.original)
        self.environment_file.unlink()
        try:
            self.environment_file.symlink_to(target)
        except OSError:
            self.skipTest("creating symlinks is unavailable; synthetic reparse test still runs")
        self.assert_rejected("reparse_path")
        self.assertEqual(target.read_bytes(), self.original)

    def test_version_mismatch_never_exports(self):
        for output in (version(release="1.91.0"), version(commit="b" * 40)):
            with self.subTest(output=output):
                self.outputs[3] = output
                self.calls.clear()
                self.assert_rejected("compiler_mismatch")
                self.assertEqual(len(self.calls), 4)

    def test_wrong_host_or_malformed_versions_never_export(self):
        for output in (version(host="x86_64-unknown-linux-gnu"), b"", b"\xff\n",
                       version() + b"release: 1.90.0\n", version() + b"\n",
                       version().replace(b"release: 1.90.0\n", b""),
                       version().replace(b"binary: rustc", b"binary: proxy"),
                       version().replace(b"rustc 1.90.0", b"rustc 1.91.0", 1),
                       version().replace(b"commit-hash: " + b"a" * 40, b"commit-hash: unknown")):
            with self.subTest(output=output):
                self.outputs[2] = output
                self.calls.clear()
                self.assert_rejected("invalid_version")
                self.assertEqual(len(self.calls), 3)

    def test_tool_failure_or_timeout_at_each_stage_leaves_environment_unchanged(self):
        for stage in range(4):
            for code in ("subprocess_timeout", "subprocess_output_limit", "subprocess_nonzero",
                         "child_cleanup_unverified"):
                with self.subTest(stage=stage, code=code):
                    self.calls.clear()

                    def fail(index):
                        if index == stage:
                            raise preflight.PreflightError(code)

                    self.assert_rejected(code, fail)
                    self.assertEqual(len(self.calls), stage + 1)

    def test_rustup_identity_change_at_each_stage_blocks_export(self):
        for stage in range(4):
            with self.subTest(stage=stage):
                self.calls.clear()

                def change(index):
                    if index == stage:
                        self.rustup.write_bytes(b"changed rustup identity or contents" + bytes([stage]))

                self.assert_rejected("file_identity_changed", change)
                self.assertEqual(len(self.calls), stage + 1)
                self.rustup.write_bytes(b"owned fake executable; never executed")

    def test_compiler_identity_change_during_either_version_check_blocks_export(self):
        for stage in (2, 3):
            with self.subTest(stage=stage):
                self.calls.clear()

                def change(index):
                    if index == stage:
                        self.compiler.write_bytes(b"changed compiler identity or contents" + bytes([stage]))

                self.assert_rejected("file_identity_changed", change)
                self.compiler.write_bytes(b"owned fake executable; never executed")

    def test_environment_change_during_each_command_is_not_overwritten(self):
        for stage in range(4):
            with self.subTest(stage=stage):
                self.environment_file.write_bytes(self.original)
                self.calls.clear()

                def change(index):
                    if index == stage:
                        self.environment_file.write_bytes(b"OTHER=changed externally\n")

                with self.assertRaisesRegex(preflight.PreflightError, "environment_identity_changed"):
                    self.execute(change)
                self.assertEqual(self.environment_file.read_bytes(), b"OTHER=changed externally\n")

    def test_environment_change_between_inspection_and_open_blocks_all_commands(self):
        inspect = preflight.FileIdentity.inspect

        def inspect_then_change(value, name=None):
            identity = inspect(value, name)
            if identity.path == self.environment_file:
                self.environment_file.write_bytes(b"EXTERNALLY=changed before open\n")
            return identity

        with mock.patch.object(preflight.FileIdentity, "inspect", side_effect=inspect_then_change):
            with self.assertRaisesRegex(preflight.PreflightError, "environment_identity_changed"):
                self.execute()
        self.assertEqual(self.calls, [])
        self.assertEqual(self.environment_file.read_bytes(), b"EXTERNALLY=changed before open\n")

    def test_total_budget_is_not_renewed_between_commands(self):
        now = [100.0]

        def advance(index):
            now[0] += preflight.TOTAL_SECONDS + 1

        with mock.patch.object(preflight.time, "monotonic", side_effect=lambda: now[0]):
            self.assert_rejected("total_timeout", advance)
        self.assertEqual(len(self.calls), 1)

    def test_change_at_final_append_boundary_is_rejected(self):
        append = preflight.append_rustc

        def change_then_append(stream, environment_file, compiler, rustup, deadline):
            self.compiler.write_bytes(b"changed immediately before append")
            return append(stream, environment_file, compiler, rustup, deadline)

        with mock.patch.object(preflight, "append_rustc", side_effect=change_then_append):
            self.assert_rejected("file_identity_changed")

    def test_delayed_final_admission_cannot_write_or_pass(self):
        now = [100.0]
        append = preflight.append_rustc

        def delayed_append(*args):
            now[0] += preflight.TOTAL_SECONDS + 1
            return append(*args)

        with mock.patch.object(preflight.time, "monotonic", side_effect=lambda: now[0]), \
                mock.patch.object(preflight, "append_rustc", side_effect=delayed_append):
            self.assert_rejected("total_timeout")

    def test_late_close_fails_even_after_a_completed_valid_append(self):
        now = [100.0]
        real_open = Path.open

        @contextmanager
        def delayed_close(path, *args, **kwargs):
            with real_open(path, *args, **kwargs) as stream:
                yield stream
            now[0] += preflight.TOTAL_SECONDS + 1

        with mock.patch.object(preflight.time, "monotonic", side_effect=lambda: now[0]), \
                mock.patch.object(Path, "open", delayed_close):
            with self.assertRaisesRegex(preflight.PreflightError, "total_timeout"):
                self.execute()
        self.assertEqual(self.environment_file.read_bytes(), self.original
                         + ("RUSTC=" + str(self.compiler.resolve()) + "\n").encode())


class PathAndAppendTests(unittest.TestCase):
    def test_local_windows_drive_forms(self):
        for path in (r"C:\Users\runner\.cargo\bin\rustup.exe", r"D:/a/_temp/env", r"\\?\C:\Rust\bin\rustc.exe"):
            with self.subTest(path=path):
                self.assertTrue(preflight.local_windows_path(path))
        for path in (r"\\server\share\rustc.exe", r"\\?\UNC\server\share\rustc.exe",
                     r"\\.\C:\rustc.exe", r"\\?\GLOBALROOT\Device\HarddiskVolume1\rustc.exe",
                     r"C:rustc.exe", r"\rustc.exe", "/tmp/rustc.exe", "rustc.exe",
                     "C:\\rustc.exe\nOTHER=value", "C:\\rustc.exe\0", r"C:\rustc.exe:stream"):
            with self.subTest(path=path):
                self.assertFalse(preflight.local_windows_path(path))

    def test_windows_remote_paths_fail_before_filesystem_probe(self):
        for path in (r"\\server\share\rustc.exe", r"\\?\UNC\server\share\rustc.exe", r"\\.\pipe\evil"):
            with self.subTest(path=path), mock.patch.object(preflight.os, "name", "nt"), \
                    mock.patch.object(preflight, "regular_info") as inspect:
                with self.assertRaisesRegex(preflight.PreflightError, "invalid_path"):
                    preflight.FileIdentity.inspect(path)
                inspect.assert_not_called()

    def test_synthetic_windows_reparse_attribute_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            path = root / "rustc.exe"
            path.write_bytes(b"fake")
            real_lstat = Path.lstat

            for reparsed in (root, path):
                def reparse(item, *args, **kwargs):
                    result = real_lstat(item, *args, **kwargs)
                    if item == reparsed:
                        return SimpleNamespace(st_mode=result.st_mode, st_file_attributes=preflight.REPARSE_POINT)
                    return result

                with self.subTest(ancestor=reparsed == root), mock.patch.object(Path, "lstat", reparse):
                    with self.assertRaisesRegex(preflight.PreflightError, "reparse_path"):
                        preflight.FileIdentity.inspect(str(path))

    def test_short_append_is_rolled_back(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            path = root / "env"
            path.write_bytes(b"ORIGINAL=value\n")
            compiler_path = root / "rustc.exe"
            compiler_path.write_bytes(b"fake")
            identity = preflight.FileIdentity.inspect(str(path))
            compiler = preflight.FileIdentity.inspect(str(compiler_path))
            with path.open("r+b", buffering=0) as stream:
                wrapped = mock.Mock(wraps=stream)
                wrapped.write.side_effect = lambda payload: stream.write(payload[:4])
                with self.assertRaisesRegex(preflight.PreflightError, "environment_write_failed"):
                    preflight.append_rustc(wrapped, identity, compiler, compiler, time.monotonic() + 10)
            self.assertEqual(path.read_bytes(), b"ORIGINAL=value\n")

    def test_identity_change_during_tail_read_blocks_append(self):
        for changed_role in ("compiler", "rustup", "environment"):
            with self.subTest(role=changed_role), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                paths = {role: root / role for role in ("compiler", "rustup", "environment")}
                for path in paths.values():
                    path.write_bytes(b"original bytes")
                identities = {role: preflight.FileIdentity.inspect(str(path)) for role, path in paths.items()}
                with paths["environment"].open("r+b", buffering=0) as stream:
                    wrapped = mock.Mock(wraps=stream)

                    def change_and_read(size):
                        paths[changed_role].write_bytes(b"externally changed bytes")
                        return stream.read(size)

                    wrapped.read.side_effect = change_and_read
                    with self.assertRaises(preflight.PreflightError):
                        preflight.append_rustc(wrapped, identities["environment"], identities["compiler"],
                                               identities["rustup"], time.monotonic() + 10)
                expected = b"externally changed bytes" if changed_role == "environment" else b"original bytes"
                self.assertEqual(paths["environment"].read_bytes(), expected)

    def test_delayed_write_rolls_back_and_never_passes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            path = root / "env"
            path.write_bytes(b"ORIGINAL=value\n")
            compiler_path = root / "rustc.exe"
            compiler_path.write_bytes(b"fake")
            identity = preflight.FileIdentity.inspect(str(path))
            compiler = preflight.FileIdentity.inspect(str(compiler_path))
            now = [100.0]
            with path.open("r+b", buffering=0) as stream:
                wrapped = mock.Mock(wraps=stream)

                def delayed_write(payload):
                    written = stream.write(payload)
                    now[0] = 102.0
                    return written

                wrapped.write.side_effect = delayed_write
                with mock.patch.object(preflight.time, "monotonic", side_effect=lambda: now[0]):
                    with self.assertRaisesRegex(preflight.PreflightError, "total_timeout"):
                        preflight.append_rustc(wrapped, identity, compiler, compiler, 101.0)
            self.assertEqual(path.read_bytes(), b"ORIGINAL=value\n")

    def test_failed_rollback_is_reported_as_unverified(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            path = root / "env"
            path.write_bytes(b"ORIGINAL=value\n")
            compiler_path = root / "rustc.exe"
            compiler_path.write_bytes(b"fake")
            identity = preflight.FileIdentity.inspect(str(path))
            compiler = preflight.FileIdentity.inspect(str(compiler_path))
            with path.open("r+b", buffering=0) as stream:
                wrapped = mock.Mock(wraps=stream)
                wrapped.write.side_effect = lambda payload: stream.write(payload[:4])
                # A no-op truncate simulates a rollback that silently failed.
                wrapped.truncate.return_value = identity.facts[3]
                with self.assertRaisesRegex(preflight.PreflightError, "environment_write_unverified"):
                    preflight.append_rustc(wrapped, identity, compiler, compiler, time.monotonic() + 10)
            self.assertEqual(path.read_bytes(), b"ORIGINAL=value\nRUST")


class BoundedProcessTests(unittest.TestCase):
    def command(self, code):
        return [sys.executable, "-I", "-S", "-c", code]

    def run_child(self, code):
        return preflight.bounded_process(self.command(code), dict(os.environ),
                                         time.monotonic() + 10)

    def test_real_child_output_is_collected_without_a_shell(self):
        self.assertEqual(self.run_child("print('bounded')"), b"bounded\n" if os.name != "nt" else b"bounded\r\n")

    def test_output_cap_is_enforced_during_capture(self):
        with mock.patch.object(preflight, "MAX_OUTPUT_BYTES", 4096):
            with self.assertRaisesRegex(preflight.PreflightError, "subprocess_output_limit"):
                self.run_child("import os; os.write(1, b'x' * 4097)")
            self.assertEqual(self.run_child("import os; os.write(1, b'x' * 4096)"), b"x" * 4096)

    def test_stderr_shares_the_same_bounded_capture(self):
        with mock.patch.object(preflight, "MAX_OUTPUT_BYTES", 4096):
            with self.assertRaisesRegex(preflight.PreflightError, "subprocess_output_limit"):
                self.run_child("import os; os.write(1, b'x' * 2048); os.write(2, b'y' * 2049)")

    def test_hanging_child_is_killed_and_reaped(self):
        with mock.patch.object(preflight, "CALL_SECONDS", 0.2):
            started = time.monotonic()
            with self.assertRaisesRegex(preflight.PreflightError, "subprocess_timeout"):
                self.run_child("import time; time.sleep(30)")
            self.assertLess(time.monotonic() - started, 5)

    def test_nonzero_child_is_rejected_without_publishing_stderr(self):
        with self.assertRaisesRegex(preflight.PreflightError, "subprocess_nonzero"):
            self.run_child("import sys; print('private path', file=sys.stderr); sys.exit(1)")

    def test_expired_total_budget_does_not_spawn(self):
        with mock.patch.object(preflight.subprocess, "Popen") as spawn:
            with self.assertRaisesRegex(preflight.PreflightError, "total_timeout"):
                preflight.bounded_process(["unused"], {}, time.monotonic() - 1)
            spawn.assert_not_called()

    def test_insufficient_cleanup_budget_does_not_spawn(self):
        with mock.patch.object(preflight.subprocess, "Popen") as spawn:
            with self.assertRaisesRegex(preflight.PreflightError, "total_timeout"):
                preflight.bounded_process(["unused"], {}, time.monotonic() + 0.5)
            spawn.assert_not_called()

    def supervise(self, root_exited, pipe_eof, cleanup_failure=None):
        process = mock.Mock()
        process.stdout.fileno.return_value = 123
        process.poll.return_value = 0 if root_exited else None
        process.returncode = 0 if root_exited else None
        if cleanup_failure == "kill":
            process.kill.side_effect = OSError("private cleanup error")
        if cleanup_failure == "wait":
            process.wait.side_effect = subprocess.TimeoutExpired("private child", 0)
        now = [100.0]

        def advance(duration):
            now[0] += duration

        read = mock.Mock(return_value=b"") if pipe_eof else mock.Mock(side_effect=BlockingIOError())
        with mock.patch.object(preflight.subprocess, "Popen", return_value=process), \
                mock.patch.object(preflight.os, "set_blocking"), \
                mock.patch.object(preflight.os, "read", read), \
                mock.patch.object(preflight.time, "monotonic", side_effect=lambda: now[0]), \
                mock.patch.object(preflight.time, "sleep", side_effect=advance), \
                mock.patch.object(preflight, "CALL_SECONDS", 0.03):
            with self.assertRaises(preflight.PreflightError) as failure:
                preflight.bounded_process(["fixed owned mock"], {}, 110.0)
        process.stdout.close.assert_called_once()
        self.assertLess(now[0], 101.0)
        return str(failure.exception), process, read

    def test_root_exit_does_not_remove_pipe_eof_deadline(self):
        code, process, _ = self.supervise(root_exited=True, pipe_eof=False)
        self.assertEqual(code, "subprocess_timeout")
        process.kill.assert_not_called()
        process.wait.assert_called_once()

    def test_pipe_eof_does_not_remove_process_deadline(self):
        code, process, read = self.supervise(root_exited=False, pipe_eof=True)
        self.assertEqual(code, "subprocess_timeout")
        read.assert_called_once()
        process.kill.assert_called_once()
        process.wait.assert_called_once()

    def test_kill_or_reap_failure_is_explicit(self):
        for stage in ("kill", "wait"):
            with self.subTest(stage=stage):
                code, process, _ = self.supervise(root_exited=False, pipe_eof=True, cleanup_failure=stage)
                self.assertEqual(code, "child_cleanup_unverified")
                process.kill.assert_called_once()


class ReceiptTests(unittest.TestCase):
    def test_errors_never_publish_raw_paths_or_child_messages(self):
        for error, code in ((OSError("private-path and raw tool error"), "preflight_error"),
                            (preflight.PreflightError("private-path"), "preflight_error"),
                            (preflight.PreflightError("subprocess_timeout"), "subprocess_timeout")):
            with self.subTest(error=type(error).__name__):
                output = io.StringIO()
                with mock.patch.object(preflight, "run", side_effect=error), redirect_stdout(output):
                    self.assertEqual(preflight.main(), 1)
                self.assertEqual(json.loads(output.getvalue()), {
                    "schema_version": 1, "suite": "windows_fixture_compiler", "result": "FAIL", "code": code,
                })
                self.assertNotIn("private-path", output.getvalue())


if __name__ == "__main__":
    unittest.main()
