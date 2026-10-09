#!/usr/bin/env python3
"""Portable regression tests for the Windows bundle boundary; no Rust build."""
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
import warnings
import zipfile
import zlib

import package_windows_bundle as bundle


COMMIT = "a" * 40
RUN_URL = bundle.REPOSITORY + "/actions/runs/123456789"


def synthetic_pe(machine=0x8664, characteristics=0x22, magic=0x20B):
    """A structurally valid tiny PE header, never executed by these tests."""
    data = bytearray(512)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<HH", data, 0x84, machine, 1)
    struct.pack_into("<HH", data, 0x94, 112, characteristics)
    struct.pack_into("<H", data, 0x98, magic)
    struct.pack_into("<H", data, 0x98 + 68, 3)
    struct.pack_into("<II", data, 0x98 + 112 + 16, 32, 384)
    data[384:416] = b"x" * 32
    return bytes(data)


def payload_fixture():
    payload = {name: (name + "\n").encode("utf-8") for name in bundle.SOURCE_FILES.values()}
    payload.update({name: synthetic_pe() for name in bundle.BINARIES})
    payload["third-party-licenses/example-1.0.0/LICENSE-MIT"] = b"Example upstream license\n"
    return payload


def custom_archive(entries, compression_level=6):
    output = io.BytesIO()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        with zipfile.ZipFile(output, "w", allowZip64=False) as archive:
            for name, content in entries:
                info = name if isinstance(name, zipfile.ZipInfo) else bundle.zip_info(name)
                archive.writestr(info, content, compresslevel=compression_level)
    return output.getvalue()


class WindowsBundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cedar-windows-bundle-test-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.payload = payload_fixture()
        self.manifest = bundle.make_manifest("0.14.0", COMMIT, RUN_URL, self.payload)
        self.archive = bundle.archive_bytes(self.payload, self.manifest)

    def entries(self, payload=None, manifest=None):
        values = dict(self.payload if payload is None else payload)
        values[bundle.MANIFEST] = bundle.canonical_json(self.manifest if manifest is None else manifest)
        return sorted(values.items())

    def test_current_entry_guides_link_only_to_packaged_local_targets(self):
        self.assertEqual(bundle.SOURCE_FILES["docs/JAVA_IMPLEMENTATIONS.md"],
                         "JAVA_IMPLEMENTATIONS.md")
        self.assertEqual(bundle.SOURCE_FILES["docs/MAVEN_DEPENDENCIES.md"], "MAVEN_DEPENDENCIES.md")
        self.assertEqual(bundle.SOURCE_FILES["docs/EXPLORER_TREE.md"], "EXPLORER_TREE.md")
        root = Path(__file__).resolve().parent.parent
        payload = dict(self.payload)
        for source in ("docs/WINDOWS_QUICKSTART.zh-CN.md", "docs/JAVA_IMPLEMENTATIONS.md", "docs/MAVEN_DEPENDENCIES.md", "docs/EXPLORER_TREE.md"):
            payload[bundle.SOURCE_FILES[source]] = (root / source).read_bytes()
        bundle.validate_entry_guide_links(payload)
        manifest = bundle.make_manifest("0.28.1", COMMIT, RUN_URL, payload)
        _, verified = bundle.verify_bytes(bundle.archive_bytes(payload, manifest))
        self.assertEqual(verified["JAVA_IMPLEMENTATIONS.md"], payload["JAVA_IMPLEMENTATIONS.md"])
        self.assertEqual(verified["MAVEN_DEPENDENCIES.md"], payload["MAVEN_DEPENDENCIES.md"])
        self.assertEqual(verified["EXPLORER_TREE.md"], payload["EXPLORER_TREE.md"])

    def test_consistent_hashes_cannot_hide_broken_entry_guide_links(self):
        for name in ("WINDOWS_QUICKSTART.zh-CN.md", "JAVA_IMPLEMENTATIONS.md"):
            for target in ("MISSING.md", "java_implementations.md", "../JAVA_IMPLEMENTATIONS.md",
                           "JAVA_IMPLEMENTATIONS.md?other=1", "JAVA%5fIMPLEMENTATIONS.md"):
                with self.subTest(name=name, target=target):
                    payload = dict(self.payload)
                    payload[name] = f"[Read guide]({target})\n".encode()
                    manifest = bundle.make_manifest("0.28.1", COMMIT, RUN_URL, payload)
                    with self.assertRaisesRegex(bundle.BundleError, "unpackaged target"):
                        bundle.verify_bytes(bundle.archive_bytes(payload, manifest))

    def test_entry_guide_fragments_and_external_links_need_no_fetch(self):
        payload = dict(self.payload)
        payload["WINDOWS_QUICKSTART.zh-CN.md"] = (
            "[Guide](JAVA_IMPLEMENTATIONS.md#what-jdt-means)\n"
            "[Section](#section) [Official](https://example.invalid/guide?q=1)\n"
            "[Manifest](BUNDLE_MANIFEST.json)\n"
        ).encode()
        manifest = bundle.make_manifest("0.28.1", COMMIT, RUN_URL, payload)
        bundle.verify_bytes(bundle.archive_bytes(payload, manifest))

    def test_bounded_malformed_inline_markers_do_not_rescan_each_suffix(self):
        for marker in (b"[", b"[a]("):
            payload = dict(self.payload)
            payload["WINDOWS_QUICKSTART.zh-CN.md"] = marker * (bundle.MAX_TEXT_BYTES // len(marker))
            bundle.validate_entry_guide_links(payload)

    def test_implementation_guide_missing_or_invalid_utf8_is_rejected(self):
        payload = dict(self.payload)
        del payload["JAVA_IMPLEMENTATIONS.md"]
        manifest = bundle.make_manifest("0.28.1", COMMIT, RUN_URL, payload)
        with self.assertRaisesRegex(bundle.BundleError, "required payload"):
            bundle.verify_bytes(bundle.archive_bytes(payload, manifest))
        payload = dict(self.payload)
        payload["JAVA_IMPLEMENTATIONS.md"] = b"\xff"
        manifest = bundle.make_manifest("0.28.1", COMMIT, RUN_URL, payload)
        with self.assertRaisesRegex(bundle.BundleError, "UTF-8 entry guide"):
            bundle.verify_bytes(bundle.archive_bytes(payload, manifest))

    def test_implementation_guide_corruption_fails_its_manifest_hash(self):
        payload = dict(self.payload)
        original = payload["JAVA_IMPLEMENTATIONS.md"]
        payload["JAVA_IMPLEMENTATIONS.md"] = b"X" + original[1:]
        with self.assertRaisesRegex(bundle.BundleError, "SHA256 mismatch: JAVA_IMPLEMENTATIONS.md"):
            bundle.verify_bytes(bundle.archive_bytes(payload, self.manifest))

    def test_build_refuses_committed_broken_entry_link_before_creating_output(self):
        root, binary_dir, _ = self.source_fixture()
        guide = root / "docs/WINDOWS_QUICKSTART.zh-CN.md"
        guide.write_text("[Missing guide](MISSING.md)\n", encoding="utf-8")
        self.git(root, "add", str(guide.relative_to(root)))
        self.git(root, "-c", "user.name=Bundle test", "-c", "user.email=test@localhost",
                 "commit", "-qm", "Synthetic broken guide")
        commit = self.git(root, "rev-parse", "HEAD").decode("ascii").strip()
        output = self.base / "must-not-exist.zip"
        with self.assertRaisesRegex(bundle.BundleError, "unpackaged target"):
            bundle.build(root, binary_dir, output, commit, RUN_URL)
        self.assertFalse(output.exists())

    def assert_invalid(self, data):
        with self.assertRaises(bundle.BundleError):
            bundle.verify_bytes(data)

    def git(self, root, *args):
        return subprocess.check_output(["git", "-C", str(root), *args], stderr=subprocess.DEVNULL)

    def source_fixture(self):
        root = self.base / "source"
        root.mkdir()
        (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.14.0"\n', encoding="utf-8")
        (root / ".gitignore").write_text("/target/\n", encoding="utf-8")
        for relative, output_name in bundle.SOURCE_FILES.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(self.payload[output_name])
        notice = "third-party-licenses/example-1.0.0/LICENSE-MIT"
        (root / notice).parent.mkdir(parents=True)
        (root / notice).write_bytes(self.payload[notice])
        self.git(root, "init", "-q")
        self.git(root, "config", "core.autocrlf", "false")
        self.git(root, "add", ".")
        self.git(root, "-c", "user.name=Bundle test", "-c", "user.email=test@localhost",
                 "commit", "-qm", "Synthetic bundle source")
        commit = self.git(root, "rev-parse", "HEAD").decode("ascii").strip()
        binary_dir = root / "target/release"
        binary_dir.mkdir(parents=True)
        for name in bundle.BINARIES:
            (binary_dir / name).write_bytes(self.payload[name])
        return root, binary_dir, commit

    def test_roundtrip_hashes_complete_inventory_and_deterministic_metadata(self):
        manifest, payload = bundle.verify_bytes(self.archive, COMMIT, RUN_URL)
        self.assertEqual(manifest, self.manifest)
        self.assertEqual(set(payload), set(self.payload) | {bundle.MANIFEST})
        self.assertEqual(self.archive, bundle.archive_bytes(self.payload, self.manifest))
        destination = self.base / "extracted"
        bundle.extract_payload(payload, destination)
        files = {p.relative_to(destination).as_posix(): p.read_bytes()
                 for p in destination.rglob("*") if p.is_file()}
        self.assertEqual(files, payload)
        for record in manifest["files"]:
            self.assertEqual(hashlib.sha256(files[record["path"]]).hexdigest(), record["sha256"])

    def test_verification_allows_different_deflate_encodings(self):
        archive = custom_archive(self.entries(), compression_level=1)
        self.assertEqual(bundle.verify_bytes(archive)[0], self.manifest)

    def test_build_only_shipping_files_and_receipt(self):
        root, binary_dir, commit = self.source_fixture()
        for name in ("cedar-agent-language-validation.exe", "cedar-mock-lsp.exe", "private.log", "jdt.jar"):
            (binary_dir / name).write_bytes(b"must not ship")
        output = self.base / "bundle.zip"
        result = bundle.build(root, binary_dir, output, commit, RUN_URL)
        self.assertEqual(result["source_commit"], commit)
        self.assertEqual(result["version"], "0.14.0")
        self.assertEqual(result["file_count"], len(self.payload) + 1)
        self.assertEqual(result["sha256"], hashlib.sha256(output.read_bytes()).hexdigest())
        manifest, payload = bundle.verify_bytes(output.read_bytes(), commit, RUN_URL)
        self.assertEqual(set(payload), set(self.payload) | {bundle.MANIFEST})
        self.assertEqual(manifest["source"]["commit_url"], bundle.REPOSITORY + "/commit/" + commit)

    def test_build_rejects_wrong_commit_dirty_source_and_untracked_source(self):
        root, binary_dir, commit = self.source_fixture()
        output = self.base / "bundle.zip"
        with self.assertRaisesRegex(bundle.BundleError, "does not match"):
            bundle.build(root, binary_dir, output, COMMIT, RUN_URL)
        (root / "new.txt").write_bytes(b"untracked")
        with self.assertRaisesRegex(bundle.BundleError, "clean"):
            bundle.build(root, binary_dir, output, commit, RUN_URL)
        (root / "new.txt").unlink()
        (root / "LICENSE-MIT").write_bytes(b"changed")
        with self.assertRaisesRegex(bundle.BundleError, "clean"):
            bundle.build(root, binary_dir, output, commit, RUN_URL)
        self.assertFalse(output.exists())

    def test_build_rejects_hidden_source_changes(self):
        root, binary_dir, commit = self.source_fixture()
        self.git(root, "update-index", "--assume-unchanged", "LICENSE-MIT")
        (root / "LICENSE-MIT").write_bytes(b"tampered despite a clean git status")
        with self.assertRaisesRegex(bundle.BundleError, "differs from HEAD"):
            bundle.build(root, binary_dir, self.base / "bundle.zip", commit, RUN_URL)

    def test_build_rejects_unapproved_tracked_license_tree_content(self):
        root, binary_dir, _ = self.source_fixture()
        path = root / "third-party-licenses/example-1.0.0/private.log"
        path.write_bytes(b"not a license")
        self.git(root, "add", ".")
        self.git(root, "-c", "user.name=Bundle test", "-c", "user.email=test@localhost",
                 "commit", "-qm", "Unapproved content")
        commit = self.git(root, "rev-parse", "HEAD").decode("ascii").strip()
        with self.assertRaisesRegex(bundle.BundleError, "Unapproved"):
            bundle.build(root, binary_dir, self.base / "bundle.zip", commit, RUN_URL)

    def test_source_crlf_is_canonicalized_only_when_it_matches_committed_blob(self):
        path = self.base / "LICENSE"
        path.write_bytes(b"first\r\nsecond\r\n")
        expected = b"first\nsecond\n"
        self.assertEqual(bundle.source_bytes(self.base, "LICENSE", bundle.blob_hash(expected)), expected)
        with self.assertRaises(bundle.BundleError):
            bundle.source_bytes(self.base, "LICENSE", "f" * 40)

    def test_invalid_or_foreign_ci_and_expected_mapping_rejected(self):
        for url in (RUN_URL + "?x=1", RUN_URL + "/", RUN_URL.replace("LLLLimbo", "other"),
                    RUN_URL.replace("https:", "http:"), bundle.REPOSITORY + "/actions/runs/0"):
            with self.subTest(url=url):
                manifest = copy.deepcopy(self.manifest)
                manifest["ci"]["run_url"] = url
                self.assert_invalid(bundle.archive_bytes(self.payload, manifest))
        with self.assertRaisesRegex(bundle.BundleError, "source commit mismatch"):
            bundle.verify_bytes(self.archive, "b" * 40)
        with self.assertRaisesRegex(bundle.BundleError, "CI run URL mismatch"):
            bundle.verify_bytes(self.archive, expected_ci=RUN_URL + "1")

    def test_missing_extra_and_duplicate_archive_entries_rejected(self):
        entries = self.entries()
        self.assert_invalid(custom_archive(entries[1:]))
        self.assert_invalid(custom_archive(entries + [("private.log", b"secret")]))
        self.assert_invalid(custom_archive(entries + [("cedar.exe", self.payload["cedar.exe"])]))
        extra = dict(self.payload)
        extra["third-party-licenses/example-1.0.0/LICENSE-APACHE"] = b"extra but unmanifested"
        self.assert_invalid(custom_archive(self.entries(extra)))

    def test_manifest_cannot_omit_required_binary_or_notice_inventory(self):
        for remove in ("cedar-agent.exe", "third-party-licenses/example-1.0.0/LICENSE-MIT"):
            payload = {key: data for key, data in self.payload.items() if key != remove}
            manifest = bundle.make_manifest("0.14.0", COMMIT, RUN_URL, payload)
            self.assert_invalid(bundle.archive_bytes(payload, manifest))

    def test_payload_hash_and_size_tampering_rejected(self):
        payload = dict(self.payload)
        payload["LICENSE-MIT"] = b"X" * len(payload["LICENSE-MIT"])
        self.assert_invalid(custom_archive(self.entries(payload)))
        manifest = copy.deepcopy(self.manifest)
        manifest["files"][0]["bytes"] += 1
        self.assert_invalid(bundle.archive_bytes(self.payload, manifest))

    def test_unsafe_names_rejected_before_extraction(self):
        for name in ("../outside", "/absolute", "C:/drive", "a\\escape", "a//b", "a/./b",
                     "third-party-licenses/CON/LICENSE", "a./file", "a /file", "a\0hidden",
                     "cedar.exe:stream", "NUL.txt", "a/..", "a/", "a/COM1.log"):
            with self.subTest(name=name):
                with self.assertRaises(bundle.BundleError):
                    bundle.safe_name(name)
                self.assert_invalid(custom_archive(self.entries() + [(name, b"bad")]))
        self.assertFalse((self.base / "outside").exists())

    def test_case_collisions_and_directory_entries_rejected(self):
        extra = "third-party-licenses/EXAMPLE-1.0.0/LICENSE-MIT"
        self.assert_invalid(custom_archive(self.entries() + [(extra, b"case collision")]))
        self.assert_invalid(custom_archive(self.entries() + [("third-party-licenses/", b"")]))

    def test_manifest_schema_duplicate_keys_and_invalid_scalars_rejected(self):
        mutations = [lambda value: value.update(extra=True),
                     lambda value: value.update(schema_version=True),
                     lambda value: value.update(unsigned=False),
                     lambda value: value.update(version="not-a-version"),
                     lambda value: value["files"][0].update(bytes=True),
                     lambda value: value["files"][0].update(sha256="A" * 64),
                     lambda value: value["files"].append(copy.deepcopy(value["files"][0])),
                     lambda value: value["source"].update(commit_url="https://example.invalid")]
        for mutate in mutations:
            value = copy.deepcopy(self.manifest)
            mutate(value)
            self.assert_invalid(bundle.archive_bytes(self.payload, value))
        data = bundle.canonical_json(self.manifest).replace(b'"schema_version": 1,',
                                                         b'"schema_version": 1, "schema_version": 1,')
        entries = [(name, data if name == bundle.MANIFEST else content) for name, content in self.entries()]
        self.assert_invalid(custom_archive(entries))

    def test_symlink_zip_permissions_and_metadata_rejected(self):
        for change in (lambda info: setattr(info, "external_attr", (stat.S_IFLNK | 0o777) << 16),
                       lambda info: setattr(info, "extra", b"\x00\x00\x00\x00"),
                       lambda info: setattr(info, "comment", b"hidden"),
                       lambda info: setattr(info, "date_time", (2026, 1, 1, 0, 0, 0)),
                       lambda info: setattr(info, "compress_type", zipfile.ZIP_STORED)):
            info = bundle.zip_info("cedar.exe")
            change(info)
            entries = [(info if name == "cedar.exe" else name, content) for name, content in self.entries()]
            self.assert_invalid(custom_archive(entries))

    def test_zip_prefix_trailer_corruption_and_hidden_header_data_rejected(self):
        for data in (b"prefix" + self.archive, self.archive + b"trailer", self.archive[:-1],
                     self.archive[:10] + b"XX" + self.archive[12:]):
            self.assert_invalid(data)
        broken = bytearray(self.archive)
        broken[30 + len(bundle.MANIFEST)] ^= 0xFF
        self.assert_invalid(bytes(broken))

    def test_hidden_bytes_after_deflate_stream_rejected(self):
        with zipfile.ZipFile(io.BytesIO(self.archive)) as archive:
            entries = archive.infolist()
        central_offset = struct.unpack_from("<I", self.archive, len(self.archive) - 6)[0]
        hidden = b"PRIVATE LOG DATA AFTER END OF DEFLATE STREAM"
        data = bytearray(self.archive[:central_offset] + hidden + self.archive[central_offset:])
        last = entries[-1]
        struct.pack_into("<I", data, last.header_offset + 18, last.compress_size + len(hidden))
        last_central = central_offset + len(hidden) + sum(46 + len(item.filename) for item in entries[:-1])
        struct.pack_into("<I", data, last_central + 20, last.compress_size + len(hidden))
        struct.pack_into("<I", data, len(data) - 6, central_offset + len(hidden))
        # Ordinary zipfile reading accepts the entry and silently discards the suffix.
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            self.assertEqual(archive.read(last.filename), self.payload[last.filename])
        self.assert_invalid(bytes(data))

    def test_inflation_bound_and_truncated_stream_rejected(self):
        compressor = zlib.compressobj(wbits=-15)
        compressed = compressor.compress(b"x" * (1024 * 1024)) + compressor.flush()
        entry = bundle.zip_info("LICENSE-MIT")
        entry.header_offset = 0
        entry.file_size = 8
        entry.compress_size = len(compressed)
        entry.CRC = zlib.crc32(b"x" * 8)
        data = b"\0" * (30 + len(entry.filename)) + compressed
        with self.assertRaisesRegex(bundle.BundleError, "DEFLATE"):
            bundle.inflate_entry(data, entry)
        entry.file_size = 1024 * 1024
        entry.compress_size -= 1
        with self.assertRaisesRegex(bundle.BundleError, "DEFLATE"):
            bundle.inflate_entry(data, entry)

    def test_central_directory_limit_checked_before_zipfile_allocation(self):
        data = bytearray(self.archive)
        struct.pack_into("<HH", data, len(data) - 14, 0xFFFF, 0xFFFF)
        with mock.patch.object(bundle.zipfile, "ZipFile", side_effect=AssertionError("must not parse")):
            self.assert_invalid(bytes(data))

    def test_pe_machine_dll_magic_and_section_truncation_rejected(self):
        invalid = [b"MZ", b"\x7fELF" + b"\0" * 512,
                   synthetic_pe(machine=0x14C), synthetic_pe(machine=0xAA64),
                   synthetic_pe(characteristics=0x2022), synthetic_pe(magic=0x10B),
                   synthetic_pe()[:400]]
        for data in invalid:
            with self.subTest(length=len(data)):
                with self.assertRaises(bundle.BundleError):
                    bundle.validate_pe(data, "test.exe")
                payload = dict(self.payload, **{"cedar.exe": data})
                manifest = bundle.make_manifest("0.14.0", COMMIT, RUN_URL, payload)
                self.assert_invalid(bundle.archive_bytes(payload, manifest))

    def test_size_and_count_limits_rejected(self):
        path = self.base / "oversize"
        path.write_bytes(b"12345")
        with self.assertRaises(bundle.BundleError):
            bundle.read_regular(path, 4)
        for name, limit in (("MAX_FILES", 2), ("MAX_TOTAL_BYTES", 16), ("MAX_BINARY_BYTES", 400),
                            ("MAX_ARCHIVE_BYTES", len(self.archive) - 1)):
            with self.subTest(limit=name), mock.patch.object(bundle, name, limit):
                self.assert_invalid(self.archive)

    def test_changed_regular_read_rejected(self):
        path = self.base / "changing"
        path.write_bytes(b"data")
        info = path.stat()
        fields = ("st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns", "st_ctime_ns")
        changed = SimpleNamespace(**{field: getattr(info, field) for field in fields})
        changed.st_mtime_ns += 1
        with mock.patch.object(bundle.os, "fstat", side_effect=[info, changed]):
            with self.assertRaisesRegex(bundle.BundleError, "changed while reading"):
                bundle.read_regular(path, 10)

    def test_windows_executable_stat_api_differences_do_not_reject_stable_file(self):
        path = self.base / "cedar.exe"
        path.write_bytes(b"data")
        info = path.stat()
        fields = ("st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns", "st_ctime_ns")
        descriptor_info = SimpleNamespace(**{field: getattr(info, field) for field in fields})
        descriptor_info.st_mode ^= 0o111
        descriptor_info.st_ctime_ns += 1
        with mock.patch.object(bundle.os, "fstat", return_value=descriptor_info):
            self.assertEqual(bundle.read_regular(path, 10), b"data")

    def test_reparse_point_detection_even_off_windows(self):
        path = self.base / "reparse"
        info = SimpleNamespace(st_mode=stat.S_IFREG | 0o644, st_file_attributes=0x400)
        with mock.patch.object(Path, "lstat", return_value=info):
            with self.assertRaisesRegex(bundle.BundleError, "reparse"):
                bundle.checked_path(path)

    def test_filesystem_symlink_and_parent_symlink_rejected(self):
        target = self.base / "target"
        target.mkdir()
        (target / "file").write_bytes(b"data")
        link = self.base / "link"
        try:
            link.symlink_to(target, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("Creating symlinks requires privileges on this Windows host")
        with self.assertRaises(bundle.BundleError):
            bundle.read_regular(link / "file", 10)
        with self.assertRaises(bundle.BundleError):
            bundle.extract_payload(self.payload, link / "extract")
        file_link = self.base / "file-link"
        file_link.symlink_to(target / "file")
        with self.assertRaises(bundle.BundleError):
            bundle.read_regular(file_link, 10)

    @unittest.skipUnless(os.name == "nt", "Windows junction regression")
    def test_windows_junction_rejected(self):
        target = self.base / "target"
        target.mkdir()
        junction = self.base / "junction"
        command = f'mklink /J "{junction}" "{target}"'
        result = subprocess.run(["cmd", "/d", "/c", command], capture_output=True)
        if result.returncode:
            self.skipTest("This Windows host does not permit junction creation")
        try:
            with self.assertRaisesRegex(bundle.BundleError, "reparse"):
                bundle.checked_path(junction)
        finally:
            junction.rmdir()

    def test_extract_and_output_never_overwrite_existing_data(self):
        destination = self.base / "existing"
        destination.mkdir()
        marker = destination / "keep"
        marker.write_bytes(b"unchanged")
        with self.assertRaises(bundle.BundleError):
            bundle.extract_payload(self.payload, destination)
        self.assertEqual(marker.read_bytes(), b"unchanged")
        with self.assertRaises(FileExistsError):
            bundle.create_file(marker, b"new")
        self.assertEqual(marker.read_bytes(), b"unchanged")

    def test_failed_extraction_rolls_back_only_new_directory(self):
        destination = self.base / "new"
        with mock.patch.object(bundle, "create_file", side_effect=OSError("write failed")):
            with self.assertRaises(OSError):
                bundle.extract_payload(self.payload, destination)
        self.assertFalse(destination.exists())

    def test_cli_rejects_invalid_archive_without_creating_extraction(self):
        path = self.base / "invalid.zip"
        path.write_bytes(self.archive + b"unexpected")
        destination = self.base / "new"
        result = subprocess.run([os.sys.executable, str(Path(bundle.__file__)), "verify", str(path),
                                 "--extract-to", str(destination)], capture_output=True, text=True,
                                timeout=30)
        self.assertEqual(result.returncode, 1)
        self.assertIn("Bundle error:", result.stderr)
        self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
