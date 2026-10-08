#!/usr/bin/env python3
"""Focused fixture/diagnostic checks; only generated temporary files are used."""
import errno
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest import mock
import zlib

import git_views_smoke as smoke


class GitViewFixtureTests(unittest.TestCase):
    def setUp(self):
        self.saved = smoke.STAGE, smoke.SUBSTAGE, smoke.ASSERTIONS
        self.directory = tempfile.TemporaryDirectory(prefix="cedar-git-fixture-unit-")
        self.root = Path(self.directory.name)
        self.content = zlib.compress(b"blob 10\0synthetic\n")

    def tearDown(self):
        smoke.STAGE, smoke.SUBSTAGE, smoke.ASSERTIONS = self.saved
        # Each fixture test owns at most two files, which may still be read-only
        # after an intentionally rejected mutation. No recursive chmod is used.
        for name in ("blob", "neighbor"):
            path = self.root / name
            if path.is_file():
                path.chmod(stat.S_IREAD | stat.S_IWRITE)
        self.directory.cleanup()

    def readonly(self, name):
        path = self.root / name
        path.write_bytes(self.content)
        path.chmod(stat.S_IREAD)
        self.assertFalse(path.stat().st_mode & stat.S_IWRITE)
        return path

    def test_exact_readonly_blob_removed_and_neighbor_unchanged(self):
        target = self.readonly("blob")
        neighbor = self.readonly("neighbor")
        before, other = target.stat(), neighbor.stat()
        if os.name == "nt":
            # Native reproduction of the old fixture defect, before the fix.
            with self.assertRaises(PermissionError) as caught:
                target.unlink()
            self.assertEqual(caught.exception.winerror, 5)
        changed = []
        original = Path.chmod

        def record(path, mode, **kwargs):
            changed.append(path)
            return original(path, mode, **kwargs)

        with mock.patch.object(Path, "chmod", record):
            smoke.remove_verified_fixture_blob(target, before, self.content)
        self.assertEqual(changed, [target])
        self.assertFalse(os.path.lexists(target))
        after = neighbor.stat()
        self.assertTrue(os.path.samestat(other, after))
        self.assertEqual((other.st_mode, other.st_size, other.st_mtime_ns),
                         (after.st_mode, after.st_size, after.st_mtime_ns))
        self.assertEqual(neighbor.read_bytes(), self.content)

    def test_different_file_identity_rejected_before_chmod(self):
        target = self.readonly("blob")
        other = self.readonly("neighbor")
        with mock.patch.object(Path, "chmod") as chmod:
            with self.assertRaisesRegex(smoke.Failure, "^promisor_remove_identity$"):
                smoke.remove_verified_fixture_blob(target, other.stat(), self.content)
            chmod.assert_not_called()
        self.assertEqual(target.read_bytes(), self.content)

    def test_different_bytes_rejected_before_chmod(self):
        target = self.readonly("blob")
        with mock.patch.object(Path, "chmod") as chmod:
            with self.assertRaisesRegex(smoke.Failure, "^promisor_remove_bytes$"):
                smoke.remove_verified_fixture_blob(target, target.stat(), b"wrong")
            chmod.assert_not_called()
        self.assertEqual(target.read_bytes(), self.content)

    def test_bytes_changed_during_chmod_are_not_deleted(self):
        target = self.readonly("blob")
        before = target.stat()
        original = Path.chmod
        altered = b"x" * len(self.content)

        def corrupt(path, mode, **kwargs):
            original(path, mode, **kwargs)
            path.write_bytes(altered)
            os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns))

        with mock.patch.object(Path, "chmod", corrupt):
            with self.assertRaisesRegex(smoke.Failure, "^promisor_remove_bytes$"):
                smoke.remove_verified_fixture_blob(target, before, self.content)
        self.assertEqual(target.read_bytes(), altered)
        self.assertEqual(smoke.SUBSTAGE, "verify_writable_identity")

    def test_failure_diagnostics_are_fixed_enums_and_integer_codes(self):
        smoke.STAGE = "missing_promisor_object"
        smoke.SUBSTAGE = "remove_fixture_blob"
        error = PermissionError(errno.EACCES, "PRIVATE_MESSAGE", "PRIVATE_PATH")
        error.winerror = 5
        result = smoke.failure_record(error)
        self.assertEqual(result["code"], "acceptance_failed")
        self.assertEqual(result["exception_kind"], "permission_error")
        self.assertEqual(result["os_errno"], errno.EACCES)
        self.assertEqual(result["winerror"], 5)
        self.assertEqual(result["substage"], "remove_fixture_blob")
        self.assertNotIn("PRIVATE", json.dumps(result))

    def test_invalid_error_codes_and_exception_text_are_not_exported(self):
        error = OSError("PRIVATE_MESSAGE")
        error.errno = "PRIVATE_ERRNO"
        error.winerror = -1
        result = smoke.failure_record(error)
        self.assertEqual(result["os_errno"], 0)
        self.assertEqual(result["winerror"], 0)
        self.assertNotIn("PRIVATE", json.dumps(result))
        result = smoke.failure_record(ValueError("PRIVATE_VALUE"))
        self.assertEqual(result["exception_kind"], "value_error")
        self.assertNotIn("PRIVATE", json.dumps(result))


if __name__ == "__main__":
    unittest.main()
