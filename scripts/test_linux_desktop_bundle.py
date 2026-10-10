#!/usr/bin/env python3
"""Bounded matched Linux desktop packaging tests; no Rust build, download or execution."""
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest import mock
import zlib

import package_linux_agent_bundle as agent_bundle
import package_linux_desktop_bundle as bundle


COMMIT = "a" * 40
RUN_URL = bundle.REPOSITORY + "/actions/runs/123456789"
BASE_ADDRESS = 0x400000
NOTICE = "third-party-licenses/example-1.0.0/LICENSE-MIT"
POSIX_ONLY = unittest.skipUnless(os.name == "posix", "Linux output modes require POSIX")


def synthetic_elf(glibc="2.34", interpreter=bundle.INTERPRETER, support_library="libgcc_s.so.1"):
    """Tiny structurally valid loader tables, deliberately never executed."""
    data = bytearray(4096)
    data[:16] = b"\x7fELF\x02\x01\x01" + bytes(9)
    struct.pack_into("<HHIQQQIHHHHHH", data, 16,
                     3, 62, 1, BASE_ADDRESS + 3000, 64, 0, 0, 64, 56, 3, 0, 0, 0)
    interp = interpreter.encode("ascii") + b"\0"
    data[256:256 + len(interp)] = interp
    strings = bytearray(b"\0")

    def add_string(value):
        offset = len(strings)
        strings.extend(value.encode("ascii") + b"\0")
        return offset

    libc = add_string("libc.so.6")
    libgcc = add_string(support_library)
    versions = [(libc, [add_string("GLIBC_2.2.5"), add_string("GLIBC_" + glibc)]),
                (libgcc, [add_string("GLIBC_2.2.5" if support_library == "libm.so.6" else "GCC_3.0")])]
    data[1024:1024 + len(strings)] = strings
    tags = [(1, libc), (1, libgcc), (5, BASE_ADDRESS + 1024), (10, len(strings)),
            (0x6ffffffe, BASE_ADDRESS + 2048), (0x6fffffff, len(versions)), (0, 0)]
    for index, pair in enumerate(tags):
        struct.pack_into("<qQ", data, 320 + index * 16, *pair)
    cursor = 2048
    for index, (library, names) in enumerate(versions):
        size = 16 + len(names) * 16
        struct.pack_into("<HHIII", data, cursor, 1, len(names), library, 16,
                         size if index + 1 < len(versions) else 0)
        for auxiliary, name in enumerate(names):
            struct.pack_into("<IHHII", data, cursor + 16 + auxiliary * 16,
                             0, 0, auxiliary + 2, name, 16 if auxiliary + 1 < len(names) else 0)
        cursor += size
    headers = [
        (1, 5, 0, BASE_ADDRESS, BASE_ADDRESS, len(data), len(data), 4096),
        (3, 4, 256, BASE_ADDRESS + 256, BASE_ADDRESS + 256, len(interp), len(interp), 1),
        (2, 6, 320, BASE_ADDRESS + 320, BASE_ADDRESS + 320, len(tags) * 16, len(tags) * 16, 8),
    ]
    for index, values in enumerate(headers):
        struct.pack_into("<IIQQQQQQ", data, 64 + index * 56, *values)
    data[3900:3914] = b"GLIBC_999.0\0xxx"
    return bytes(data)


def payload_fixture():
    payload = {name: (name + "\n").encode("utf-8") for name in bundle.SOURCE_FILES.values()}
    payload[bundle.AGENT] = synthetic_elf()
    payload[bundle.DESKTOP] = synthetic_elf(support_library="libm.so.6")
    payload[NOTICE] = b"Example upstream license\n"
    return payload


def custom_archive(entries, trailer=bytes(1024)):
    raw = bytearray()
    for name, content in entries:
        if isinstance(name, tarfile.TarInfo):
            name.size = len(content)
            header = name.tobuf(tarfile.USTAR_FORMAT, encoding="ascii", errors="strict")
        else:
            header = bundle.tar_header(name, len(content))
        raw.extend(header)
        raw.extend(content)
        raw.extend(bytes(-len(content) % 512))
    raw.extend(trailer)
    return bundle.gzip_bytes(raw)


class LinuxDesktopBundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cedar-linux-desktop-test-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.payload = payload_fixture()
        self.manifest = bundle.make_manifest("0.41.0", COMMIT, RUN_URL, self.payload)
        self.archive = bundle.archive_bytes(self.payload, self.manifest)

    def entries(self, payload=None, manifest=None):
        values = dict(self.payload if payload is None else payload)
        values[bundle.MANIFEST] = bundle.canonical_json(self.manifest if manifest is None else manifest)
        return sorted(values.items())

    def assert_invalid(self, data, pattern=None):
        with self.assertRaisesRegex(bundle.BundleError, pattern or ".+"):
            bundle.verify_bytes(data)

    def mutate_elf(self, offset, value, pattern=None):
        data = bytearray(synthetic_elf())
        data[offset:offset + len(value)] = value
        with self.assertRaisesRegex(bundle.BundleError, pattern or ".+"):
            bundle.inspect_elf(data)

    def git(self, root, *arguments):
        return subprocess.check_output(["git", "-C", str(root), *arguments], stderr=subprocess.DEVNULL)

    def source_fixture(self):
        root = self.base / "source"
        root.mkdir()
        (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.41.0"\n', encoding="utf-8")
        (root / ".gitignore").write_text("/target/\n", encoding="utf-8")
        for source, target in bundle.SOURCE_FILES.items():
            path = root / source
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(self.payload[target])
        (root / NOTICE).parent.mkdir(parents=True)
        (root / NOTICE).write_bytes(self.payload[NOTICE])
        self.git(root, "init", "-q")
        self.git(root, "config", "core.autocrlf", "false")
        self.git(root, "add", ".")
        self.git(root, "-c", "user.name=Bundle test", "-c", "user.email=test@localhost",
                 "commit", "-qm", "Synthetic bundle sources")
        commit = self.git(root, "rev-parse", "HEAD").decode("ascii").strip()
        binary_dir = root / "target/release"
        binary_dir.mkdir(parents=True)
        for name in bundle.BINARIES:
            (binary_dir / name).write_bytes(self.payload[name])
            (binary_dir / name).chmod(0o755)
        return root, binary_dir, commit

    def test_archive_roundtrip_and_deterministic_bytes(self):
        manifest, payload = bundle.verify_bytes(self.archive, COMMIT, RUN_URL)
        self.assertEqual(manifest, self.manifest)
        self.assertEqual(payload, dict(self.entries()))
        self.assertEqual(self.archive, bundle.archive_bytes(self.payload, self.manifest))
        with tarfile.open(fileobj=io.BytesIO(self.archive), mode="r:gz") as archive:
            for info in archive:
                self.assertTrue(info.isfile())
                self.assertEqual(info.mode, 0o755 if info.name in bundle.BINARIES else 0o644)
                self.assertEqual((info.uid, info.gid, info.mtime, info.uname, info.gname), (0, 0, 0, "", ""))

    def test_measured_abi_ignores_unreferenced_string_decoys(self):
        abi = bundle.inspect_elf(synthetic_elf())
        self.assertEqual(abi["required_glibc"], "2.34")
        self.assertEqual(abi["needed"], ["libc.so.6", "libgcc_s.so.1"])
        self.assertEqual(abi["interpreter"], bundle.INTERPRETER)
        self.assertEqual(abi["version_requirements"], [
            {"library": "libc.so.6", "versions": ["GLIBC_2.2.5", "GLIBC_2.34"]},
            {"library": "libgcc_s.so.1", "versions": ["GCC_3.0"]}])
        self.assertEqual(bundle.inspect_elf(synthetic_elf(glibc="2.9"))["required_glibc"], "2.9")
        self.assertEqual(bundle.inspect_elf(synthetic_elf(glibc="2.39"))["required_glibc"], "2.39")

    def test_libm_is_reviewed_for_desktop_only_and_agent_contract_is_unchanged(self):
        gui = self.payload[bundle.DESKTOP]
        self.assertEqual(bundle.inspect_elf(gui, bundle.DESKTOP)["needed"],
                         ["libc.so.6", "libm.so.6"])
        for inspector in (agent_bundle.inspect_elf,
                          lambda data: bundle.inspect_elf(data, bundle.AGENT)):
            with self.assertRaisesRegex(bundle.BundleError, "Unexpected ELF dependency"):
                inspector(gui)
        self.assertNotIn("libm.so.6", agent_bundle.SYSTEM_LIBRARIES)
        with self.assertRaisesRegex(bundle.BundleError, "Unknown"):
            bundle.inspect_elf(gui, "cedar-client-bundle-probe")

    def test_agent_only_and_desktop_archives_are_distinct(self):
        import test_linux_agent_bundle as agent_tests
        payload = agent_tests.payload_fixture()
        manifest = agent_bundle.make_manifest("0.41.0", COMMIT, RUN_URL, payload)
        self.assert_invalid(agent_bundle.archive_bytes(payload, manifest))
        with self.assertRaises(bundle.BundleError):
            agent_bundle.verify_bytes(self.archive)
        agent_bundle.verify_bytes(agent_bundle.archive_bytes(payload, manifest))

    def test_paired_abi_inventory_and_both_binary_requirements_are_exact(self):
        for name in bundle.BINARIES:
            manifest = copy.deepcopy(self.manifest)
            manifest["abi"][name]["required_glibc"] = "2.2.5"
            with self.subTest(binary=name, mutation="manifest"):
                self.assert_invalid(bundle.archive_bytes(self.payload, manifest), "Measured ELF ABI")
            for bad_binary in (synthetic_elf(glibc="2.40"),
                               synthetic_elf(interpreter="/lib/ld-musl-x86_64.so.1"),
                               b"MZ" + bytes(4094)):
                payload = dict(self.payload)
                payload[name] = bad_binary
                manifest = copy.deepcopy(self.manifest)
                record = next(item for item in manifest["files"] if item["path"] == name)
                record.update(bytes=len(bad_binary), sha256=hashlib.sha256(bad_binary).hexdigest())
                with self.subTest(binary=name, mutation="ELF"):
                    self.assert_invalid(bundle.archive_bytes(payload, manifest))
        for abi in ({bundle.DESKTOP: self.manifest["abi"][bundle.DESKTOP]},
                    {**self.manifest["abi"], "probe": {}}, []):
            manifest = copy.deepcopy(self.manifest)
            manifest["abi"] = abi
            self.assert_invalid(bundle.archive_bytes(self.payload, manifest), "ABI inventory")

    def test_pair_cannot_swap_binary_bytes_without_detection(self):
        payload = dict(self.payload)
        payload[bundle.DESKTOP], payload[bundle.AGENT] = payload[bundle.AGENT], payload[bundle.DESKTOP]
        self.assert_invalid(bundle.archive_bytes(payload, self.manifest), "SHA256 mismatch")
        # Even a rewritten self-consistent hash cannot broaden the agent ABI.
        with self.assertRaisesRegex(bundle.BundleError, "Unexpected ELF dependency"):
            bundle.make_manifest("0.41.0", COMMIT, RUN_URL, payload)

    def test_unsupported_glibc_and_interpreter_are_rejected(self):
        for version in ("2.40", "PRIVATE", "ABI_DT_RELR"):
            with self.subTest(version=version), self.assertRaises(bundle.BundleError):
                bundle.inspect_elf(synthetic_elf(glibc=version))
        with self.assertRaisesRegex(bundle.BundleError, "interpreter"):
            bundle.inspect_elf(synthetic_elf(interpreter="/lib/ld-musl-x86_64.so.1"))

    def test_unreviewed_gui_and_third_party_dependencies_are_rejected(self):
        for library in ("libX11.so.6", "libjvm.so", "libcrypto.so.3", "libGL.so.1", "libwayland-client.so.0"):
            with self.subTest(library=library), self.assertRaisesRegex(bundle.BundleError, "Unexpected ELF dependency"):
                bundle.inspect_elf(synthetic_elf(support_library=library))
        with self.assertRaisesRegex(bundle.BundleError, "Unsafe ELF"):
            bundle.inspect_elf(synthetic_elf(support_library="/lib/foreign.so"))

    def test_wrong_or_truncated_elf_headers_are_rejected(self):
        for data in (b"", b"MZ" + bytes(4094), synthetic_elf()[:63], synthetic_elf()[:1000]):
            with self.subTest(length=len(data)), self.assertRaises(bundle.BundleError):
                bundle.inspect_elf(data)
        for offset, value in ((4, b"\x01"), (5, b"\x02"), (7, b"\x09"), (8, b"\x01"),
                              (16, struct.pack("<H", 1)), (18, struct.pack("<H", 183)),
                              (32, struct.pack("<Q", 4096)), (56, struct.pack("<H", 65))):
            with self.subTest(offset=offset):
                self.mutate_elf(offset, value)

    def test_dynamic_mapping_must_match_runtime_address(self):
        self.mutate_elf(64 + 112 + 16, struct.pack("<Q", BASE_ADDRESS + 1024), "load mapping")
        self.mutate_elf(64 + 112 + 40, struct.pack("<Q", 128), "file-backed")
        self.mutate_elf(320 + 2 * 16 + 8, struct.pack("<Q", BASE_ADDRESS + 4095), "Unmapped")
        # A dynamic table outside the sole file-backed mapping is rejected.
        data = bytearray(synthetic_elf())
        struct.pack_into("<IIQQQQQQ", data, 64, 1, 5, 0, BASE_ADDRESS, BASE_ADDRESS, 256, 256, 1)
        with self.assertRaises(bundle.BundleError):
            bundle.inspect_elf(data)
        # Preserve the interpreter in a later slot and add an overlapping fourth
        # program header. Address translation must never choose the first match.
        data = bytearray(synthetic_elf())
        interpreter = bundle.INTERPRETER.encode() + b"\0"
        data[480:480 + len(interpreter)] = interpreter
        struct.pack_into("<H", data, 56, 4)
        struct.pack_into("<IIQQQQQQ", data, 64 + 56, 3, 4, 480,
                         BASE_ADDRESS + 480, BASE_ADDRESS + 480, len(interpreter), len(interpreter), 1)
        struct.pack_into("<IIQQQQQQ", data, 64 + 168, 1, 4, 0,
                         BASE_ADDRESS, BASE_ADDRESS, len(data), len(data), 4096)
        with self.assertRaisesRegex(bundle.BundleError, "ambiguous"):
            bundle.inspect_elf(data)

    def test_dynamic_null_singletons_and_custom_loader_paths_are_rejected(self):
        self.mutate_elf(320 + 6 * 16, struct.pack("<qQ", 21, 0), "Missing ELF")
        self.mutate_elf(320 + 6 * 16, struct.pack("<qQ", 0, 1), "DT_NULL")
        self.mutate_elf(320 + 3 * 16, struct.pack("<q", 5), "Duplicate ELF dynamic tag")
        for tag in (15, 29, 0x6ffffefb, 0x6ffffefc, 0x7ffffffd, 0x7fffffff):
            with self.subTest(tag=tag):
                self.mutate_elf(320 + 3 * 16, struct.pack("<q", tag), "custom search")

    def test_dynamic_strings_dependencies_and_version_chains_are_bounded(self):
        self.mutate_elf(320 + 16 + 8, struct.pack("<Q", 1), "Duplicate dependencies")
        self.mutate_elf(320 + 8, struct.pack("<Q", 9999), "string offset")
        self.mutate_elf(320 + 3 * 16 + 8, struct.pack("<Q", 4), "dynamic string")
        self.mutate_elf(320 + 5 * 16 + 8, struct.pack("<Q", 65), "requirement count")
        self.mutate_elf(2048, struct.pack("<H", 2), "requirement record")
        self.mutate_elf(2048 + 2, struct.pack("<H", 257), "requirement record")
        self.mutate_elf(2048 + 12, struct.pack("<I", 0), "requirement chain")
        self.mutate_elf(2048 + 16 + 12, struct.pack("<I", 0), "auxiliary chain")
        self.mutate_elf(2048 + 32 + 12, struct.pack("<I", 16), "auxiliary chain")

    def test_abi_manifest_mismatch_is_rejected_with_consistent_payload_hashes(self):
        for field, value in (("required_glibc", "2.2.5"), ("needed", ["libc.so.6"]),
                             ("interpreter", "/different-loader"), ("machine", "EM_AARCH64"),
                             ("version_requirements", [])):
            manifest = copy.deepcopy(self.manifest)
            manifest["abi"][bundle.DESKTOP][field] = value
            with self.subTest(field=field):
                self.assert_invalid(bundle.archive_bytes(self.payload, manifest), "Measured ELF ABI")

    def test_payload_corruption_and_source_blob_mismatch(self):
        for name in (*bundle.BINARIES, bundle.GUIDE, NOTICE):
            payload = dict(self.payload)
            payload[name] = b"X" + payload[name][1:]
            with self.subTest(name=name):
                self.assert_invalid(bundle.archive_bytes(payload, self.manifest), "SHA256 mismatch")
        manifest = copy.deepcopy(self.manifest)
        next(item for item in manifest["files"] if item["path"] == bundle.GUIDE)["source"]["git_blob"] = "b" * 40
        self.assert_invalid(bundle.archive_bytes(self.payload, manifest), "Source blob mismatch")

    def test_source_document_linkage_is_exact(self):
        record = next(item for item in self.manifest["files"] if item["path"] == bundle.GUIDE)
        self.assertEqual(record["source"], {"path": "docs/" + bundle.GUIDE,
                         "git_blob": bundle.blob_hash(self.payload[bundle.GUIDE]),
                         "blob_url": f"{bundle.REPOSITORY}/blob/{COMMIT}/docs/{bundle.GUIDE}"})
        for field, value in (("path", bundle.GUIDE), ("git_blob", "bad"),
                             ("blob_url", f"{bundle.REPOSITORY}/blob/main/docs/{bundle.GUIDE}")):
            manifest = copy.deepcopy(self.manifest)
            next(item for item in manifest["files"] if item["path"] == bundle.GUIDE)["source"][field] = value
            with self.subTest(field=field):
                self.assert_invalid(bundle.archive_bytes(self.payload, manifest), "source document linkage")

    def test_commit_ci_and_platform_constraints(self):
        with self.assertRaisesRegex(bundle.BundleError, "source commit mismatch"):
            bundle.verify_bytes(self.archive, "b" * 40)
        with self.assertRaisesRegex(bundle.BundleError, "CI run URL mismatch"):
            bundle.verify_bytes(self.archive, expected_ci=RUN_URL + "0")
        for field, value in (("target", "aarch64-unknown-linux-gnu"), ("unsigned", False),
                             ("features", "windows-language-validation"), ("baseline", {}),
                             ("product", "cedar-agent"), ("schema_version", True)):
            manifest = copy.deepcopy(self.manifest)
            manifest[field] = value
            with self.subTest(field=field):
                self.assert_invalid(bundle.archive_bytes(self.payload, manifest), "Unsupported")

    def test_invalid_or_duplicate_json_and_noncanonical_manifest(self):
        original = bundle.canonical_json(self.manifest)
        for data in (b"\xff", b"[]", original.replace(b'"abi": {', b'"abi": {}, "abi": {', 1),
                     json.dumps(self.manifest).encode(), b"[" * 2000):
            entries = [(name, data if name == bundle.MANIFEST else content) for name, content in self.entries()]
            with self.subTest(prefix=data[:20]):
                self.assert_invalid(custom_archive(entries))

    def test_missing_or_extra_members_and_inventory_records(self):
        for omitted in (*bundle.BINARIES, bundle.GUIDE, bundle.MANIFEST, NOTICE):
            entries = [(name, data) for name, data in self.entries() if name != omitted]
            with self.subTest(omitted=omitted):
                self.assert_invalid(custom_archive(entries))
        for extra in ("cedar-client-bundle-probe", "cedar-agent-language-validation", "libjvm.so",
                      "java", "jdt.jar", "CJK.ttf", "fixture.sh", "docs/README.md"):
            with self.subTest(extra=extra):
                self.assert_invalid(custom_archive(sorted(self.entries() + [(extra, b"unrequested")])), "Unexpected")
        manifest = copy.deepcopy(self.manifest)
        manifest["files"] = [item for item in manifest["files"] if item["path"] != NOTICE]
        self.assert_invalid(bundle.archive_bytes(self.payload, manifest), "no third-party")

    def test_duplicate_case_colliding_and_unsorted_members(self):
        self.assert_invalid(custom_archive(sorted(self.entries() + [(NOTICE, self.payload[NOTICE])])), "Duplicate")
        self.assert_invalid(custom_archive(list(reversed(self.entries()))), "unsorted")
        # Different case is not an allowed fixed path even on a case-sensitive host.
        self.assert_invalid(custom_archive(sorted(self.entries() + [("CEDAR-AGENT", synthetic_elf())])))
        manifest = copy.deepcopy(self.manifest)
        manifest["files"].append(copy.deepcopy(manifest["files"][-1]))
        self.assert_invalid(bundle.archive_bytes(self.payload, manifest), "Duplicate")

    def test_traversal_absolute_and_ambiguous_paths(self):
        for name in ("../cedar-agent", "/cedar-agent", "./cedar-agent", "a//b", "a/../b",
                     "a\\b", "C:/cedar-agent", "a.", "NUL", "third-party-licenses/../LICENSE"):
            with self.subTest(name=name):
                self.assert_invalid(custom_archive([(name, b"x")] + self.entries()))

    def test_modes_ownership_time_and_other_tar_metadata_are_exact(self):
        for field, value in (("mode", 0o644), ("mode", 0o4755), ("mode", 0o777),
                             ("uid", 1), ("gid", 1), ("mtime", 1), ("uname", "root"),
                             ("gname", "root"), ("linkname", "elsewhere")):
            info = tarfile.TarInfo(bundle.AGENT)
            info.mode = 0o755
            setattr(info, field, value)
            entries = [(info if name == bundle.AGENT else name, data) for name, data in self.entries()]
            with self.subTest(field=field, value=value):
                self.assert_invalid(custom_archive(entries), "metadata")
        info = tarfile.TarInfo(bundle.GUIDE)
        info.mode = 0o755
        self.assert_invalid(custom_archive([(info if name == bundle.GUIDE else name, data)
                                            for name, data in self.entries()]), "metadata")
        raw = bytearray(zlib.decompress(self.archive, 31))
        raw[329:337] = b"0000001\0"  # tarfile normalizes devmajor for regular files.
        raw[148:156] = b" " * 8
        raw[148:156] = ("%06o\0 " % sum(raw[:512])).encode("ascii")
        self.assert_invalid(bundle.gzip_bytes(raw), "metadata")

    def test_links_special_files_and_extended_tar_records_are_rejected(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.DIRTYPE, tarfile.CHRTYPE,
                     tarfile.BLKTYPE, tarfile.FIFOTYPE, tarfile.XHDTYPE, tarfile.XGLTYPE,
                     tarfile.GNUTYPE_LONGNAME, tarfile.GNUTYPE_LONGLINK, tarfile.GNUTYPE_SPARSE):
            info = tarfile.TarInfo(bundle.AGENT)
            info.type, info.mode = kind, 0o755
            entries = [(info if name == bundle.AGENT else name, data) for name, data in self.entries()]
            with self.subTest(kind=kind):
                self.assert_invalid(custom_archive(entries))

    def test_tar_nonzero_padding_trailer_or_hidden_data_is_rejected(self):
        for trailer in (b"", bytes(512), bytes(1536), bytes(1024) + b"hidden"):
            with self.subTest(trailer=len(trailer)):
                self.assert_invalid(custom_archive(self.entries(), trailer))
        raw = bytearray(zlib.decompress(self.archive, 31))
        first_size = len(self.entries()[0][1])
        raw[512 + first_size] = 1
        self.assert_invalid(bundle.gzip_bytes(raw), "padding")
        raw = bytearray(zlib.decompress(self.archive, 31))
        raw[0] ^= 1
        self.assert_invalid(bundle.gzip_bytes(raw), "tar header")

    def test_gzip_trailing_members_headers_corruption_and_bombs_are_rejected(self):
        for data in (self.archive + b"extra", self.archive + self.archive, self.archive[:-1],
                     self.archive[:-8] + bytes(8), b"" , self.archive[:10]):
            with self.subTest(length=len(data)):
                self.assert_invalid(data)
        for index in (3, 4, 8, 9):
            data = bytearray(self.archive)
            data[index] ^= 1
            self.assert_invalid(data, "header")
        with mock.patch.object(bundle, "MAX_TAR_BYTES", 1024):
            self.assert_invalid(bundle.gzip_bytes(bytes(1025)), "oversized")
        with mock.patch.object(bundle, "MAX_ARCHIVE_BYTES", 64):
            self.assert_invalid(self.archive, "oversized")
        with mock.patch.object(bundle, "MAX_FILES", 2):
            self.assert_invalid(self.archive, "Too many")
        with mock.patch.object(bundle, "MAX_TEXT_BYTES", 16):
            self.assert_invalid(self.archive, "oversized tar member")
        with mock.patch.object(bundle, "MAX_BINARY_BYTES", 2048):
            self.assert_invalid(self.archive, "oversized tar member")
        with mock.patch.object(bundle, "MAX_TOTAL_BYTES", 1024):
            self.assert_invalid(self.archive, "total size limit")

    def test_guide_links_are_checked_after_hash_validation(self):
        for target in ("MISSING.md", "../LICENSE-MIT", "LICENSE%2dMIT", "LICENSE-MIT?download=1"):
            payload = dict(self.payload)
            payload[bundle.GUIDE] = f"[Guide]({target})\n".encode()
            manifest = bundle.make_manifest("0.41.0", COMMIT, RUN_URL, payload)
            with self.subTest(target=target):
                self.assert_invalid(bundle.archive_bytes(payload, manifest), "unpackaged target")
        payload = dict(self.payload)
        payload[bundle.GUIDE] = ("[License](LICENSE-MIT#license) [Manifest](BUNDLE_MANIFEST.json) "
                                 "[Section](#section) [Official](https://example.invalid/path)\n").encode()
        bundle.verify_bytes(bundle.archive_bytes(payload, bundle.make_manifest("0.41.0", COMMIT, RUN_URL, payload)))
        payload[bundle.GUIDE] = b"\xff"
        self.assert_invalid(bundle.archive_bytes(payload, bundle.make_manifest("0.41.0", COMMIT, RUN_URL, payload)), "UTF-8")

    def test_current_guide_links_target_packaged_files(self):
        root = Path(__file__).resolve().parent.parent
        payload = dict(self.payload)
        for name in ("MAVEN_PROJECTS.md", "MAVEN_DEPENDENCIES.md"):
            self.assertEqual(bundle.SOURCE_FILES["docs/" + name], name)
            payload[name] = (root / "docs" / name).read_bytes()
        payload[bundle.GUIDE] = (root / "docs" / bundle.GUIDE).read_bytes()
        manifest = bundle.make_manifest("0.41.0", COMMIT, RUN_URL, payload)
        bundle.verify_bytes(bundle.archive_bytes(payload, manifest))

    @POSIX_ONLY
    def test_fresh_unicode_extraction_rechecks_inventory_hashes_and_modes(self):
        _, payload = bundle.verify_bytes(self.archive)
        destination = self.base / "解压 空间 cedar agent"
        previous_umask = os.umask(0o077)
        try:
            bundle.extract_payload(payload, destination)
        finally:
            os.umask(previous_umask)
        bundle.verify_extracted(payload, destination)
        for name, content in payload.items():
            path = destination / name
            self.assertEqual(path.read_bytes(), content)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), bundle.file_mode(name))
        with self.assertRaisesRegex(bundle.BundleError, "already exists"):
            bundle.extract_payload(payload, destination)

    @POSIX_ONLY
    def test_postflight_rejects_extra_missing_changed_files_modes_and_directories(self):
        _, payload = bundle.verify_bytes(self.archive)
        for mutation in ("extra", "missing", "bytes", "mode", "directory", "symlink", "hardlink"):
            destination = self.base / mutation
            bundle.extract_payload(payload, destination)
            guide = destination / bundle.GUIDE
            if mutation == "extra":
                (destination / "extra").write_bytes(b"x")
            elif mutation == "missing":
                guide.unlink()
            elif mutation == "bytes":
                guide.write_bytes(b"changed")
            elif mutation == "mode":
                guide.chmod(0o755)
            elif mutation == "directory":
                (destination / "empty-extra-directory").mkdir()
            elif mutation == "symlink":
                guide.unlink()
                guide.symlink_to(destination / "LICENSE-MIT")
            elif mutation == "hardlink":
                os.link(guide, self.base / "outside-hardlink")
            with self.subTest(mutation=mutation), self.assertRaises(bundle.BundleError):
                bundle.verify_extracted(payload, destination)

    @POSIX_ONLY
    def test_extraction_rejects_symlink_parents_and_cleans_up_failures(self):
        _, payload = bundle.verify_bytes(self.archive)
        (self.base / "link").symlink_to(self.base, target_is_directory=True)
        with self.assertRaisesRegex(bundle.BundleError, "Symlink"):
            bundle.extract_payload(payload, self.base / "link" / "output")
        destination = self.base / "failed"
        with mock.patch.object(bundle, "verify_extracted", side_effect=bundle.BundleError("injected failure")):
            with self.assertRaisesRegex(bundle.BundleError, "injected"):
                bundle.extract_payload(payload, destination)
        self.assertFalse(destination.exists())

    @POSIX_ONLY
    def test_build_anchors_source_docs_to_git_and_never_overwrites_output(self):
        root, binary_dir, commit = self.source_fixture()
        (binary_dir / "cedar-client-bundle-probe").write_bytes(b"must not be included")
        (binary_dir / "cedar-agent-language-validation").write_bytes(b"must not be included")
        output = self.base / "bundle.tar.gz"
        with mock.patch.object(bundle, "require_build_host"):
            result = bundle.build(root, binary_dir, output, commit, RUN_URL)
            with self.assertRaises(FileExistsError):
                bundle.build(root, binary_dir, output, commit, RUN_URL)
        manifest, payload = bundle.verify_bytes(output.read_bytes(), commit, RUN_URL)
        bundle.verify_source(root, manifest, payload)
        self.assertEqual(result["sha256"], hashlib.sha256(output.read_bytes()).hexdigest())
        self.assertEqual(result["abi"][bundle.AGENT]["required_glibc"], "2.34")
        self.assertEqual(set(payload), set(self.payload) | {bundle.MANIFEST})

    @POSIX_ONLY
    def test_build_cargo_hardlinked_inputs_extract_as_independent_regular_files(self):
        root, binary_dir, commit = self.source_fixture()
        for name in bundle.BINARIES:
            os.link(binary_dir / name, binary_dir / (name + "-deps-link"))
        output = self.base / "cargo-pair.tar.gz"
        with mock.patch.object(bundle, "require_build_host"):
            bundle.build(root, binary_dir, output, commit, RUN_URL)
        _, payload = bundle.verify_bytes(output.read_bytes())
        destination = self.base / "Cargo 配对 新目录"
        bundle.extract_payload(payload, destination)
        for name in bundle.BINARIES:
            self.assertEqual((destination / name).stat().st_nlink, 1)

    @POSIX_ONLY
    def test_build_rejects_missing_nonexecutable_and_symlinked_pair_members(self):
        root, binary_dir, commit = self.source_fixture()
        output = self.base / "invalid-pair.tar.gz"
        for name in bundle.BINARIES:
            path = binary_dir / name
            for mutation in ("missing", "mode", "symlink", "directory"):
                if path.exists() or path.is_symlink():
                    path.unlink()
                if mutation == "mode":
                    path.write_bytes(self.payload[name])
                    path.chmod(0o644)
                elif mutation == "symlink":
                    path.symlink_to(binary_dir / (bundle.AGENT if name == bundle.DESKTOP else bundle.DESKTOP))
                elif mutation == "directory":
                    path.mkdir()
                with self.subTest(binary=name, mutation=mutation), mock.patch.object(bundle, "require_build_host"):
                    with self.assertRaises(bundle.BundleError):
                        bundle.build(root, binary_dir, output, commit, RUN_URL)
                self.assertFalse(output.exists())
                if path.is_dir():
                    path.rmdir()
            path.write_bytes(self.payload[name])
            path.chmod(0o755)

    def test_source_root_rejects_changed_documents_inventory_and_version(self):
        root, _, commit = self.source_fixture()
        manifest = bundle.make_manifest("0.41.0", commit, RUN_URL, self.payload)
        _, payload = bundle.verify_bytes(bundle.archive_bytes(self.payload, manifest))
        bundle.verify_source(root, manifest, payload)
        changed = dict(self.payload)
        changed[NOTICE] = b"fabricated but internally hash-consistent notice\n"
        forged = bundle.make_manifest("0.41.0", commit, RUN_URL, changed)
        _, changed = bundle.verify_bytes(bundle.archive_bytes(changed, forged))
        with self.assertRaisesRegex(bundle.BundleError, "committed blob"):
            bundle.verify_source(root, forged, changed)
        changed = dict(self.payload)
        changed["third-party-licenses/extra-1.0/LICENSE"] = b"not in source\n"
        forged = bundle.make_manifest("0.41.0", commit, RUN_URL, changed)
        _, changed = bundle.verify_bytes(bundle.archive_bytes(changed, forged))
        with self.assertRaisesRegex(bundle.BundleError, "inventory"):
            bundle.verify_source(root, forged, changed)
        forged = copy.deepcopy(manifest)
        forged["version"] = "0.34.0"
        with self.assertRaisesRegex(bundle.BundleError, "Cargo version"):
            bundle.verify_source(root, forged, payload)
        (root / "LICENSE-MIT").write_bytes(b"dirty\n")
        with self.assertRaisesRegex(bundle.BundleError, "clean"):
            bundle.verify_source(root, manifest, payload)

    def test_dirty_source_build_rejection_is_platform_independent(self):
        root, binary_dir, commit = self.source_fixture()
        output = self.base / "dirty-source-must-not-exist.tar.gz"
        (root / "LICENSE-MIT").write_bytes(b"dirty source")
        with mock.patch.object(bundle, "require_build_host"):
            with self.assertRaisesRegex(bundle.BundleError, "clean"):
                bundle.build(root, binary_dir, output, commit, RUN_URL)
        self.assertFalse(output.exists())

    def test_invalid_pair_elf_payloads_are_rejected_on_every_host(self):
        for name in bundle.BINARIES:
            payload = dict(self.payload)
            payload[name] = b"not an ELF"
            with self.subTest(binary=name), self.assertRaisesRegex(bundle.BundleError, "ELF"):
                bundle.make_manifest("0.41.0", COMMIT, RUN_URL, payload)

    def test_broken_guide_payload_is_rejected_on_every_host(self):
        payload = dict(self.payload)
        payload[bundle.GUIDE] = b"[Missing](NOT-PACKAGED.md)\n"
        manifest = bundle.make_manifest("0.41.0", COMMIT, RUN_URL, payload)
        self.assert_invalid(bundle.archive_bytes(payload, manifest), "unpackaged")

    @POSIX_ONLY
    def test_build_rejects_dirty_source_invalid_binary_and_broken_links_before_output(self):
        root, binary_dir, commit = self.source_fixture()
        output = self.base / "must-not-exist.tar.gz"
        (binary_dir / bundle.AGENT).write_bytes(b"not an ELF")
        (binary_dir / bundle.DESKTOP).chmod(0o644)
        with mock.patch.object(bundle, "require_build_host"):
            with self.assertRaisesRegex(bundle.BundleError, "regular 0755"):
                bundle.build(root, binary_dir, output, commit, RUN_URL)
        self.assertFalse(output.exists())
        (binary_dir / bundle.DESKTOP).chmod(0o755)
        with mock.patch.object(bundle, "require_build_host"):
            with self.assertRaisesRegex(bundle.BundleError, "ELF"):
                bundle.build(root, binary_dir, output, commit, RUN_URL)
        for name in bundle.BINARIES:
            (binary_dir / name).write_bytes(self.payload[name])
            (binary_dir / name).chmod(0o755)
        (root / "LICENSE-MIT").write_bytes(b"dirty source")
        with mock.patch.object(bundle, "require_build_host"):
            with self.assertRaisesRegex(bundle.BundleError, "clean"):
                bundle.build(root, binary_dir, output, commit, RUN_URL)
        self.git(root, "checkout", "--", "LICENSE-MIT")
        (root / "docs" / bundle.GUIDE).write_text("[Missing](NOT-PACKAGED.md)\n", encoding="utf-8")
        self.git(root, "add", ".")
        self.git(root, "-c", "user.name=Bundle test", "-c", "user.email=test@localhost", "commit", "-qm", "Broken guide")
        commit = self.git(root, "rev-parse", "HEAD").decode("ascii").strip()
        with mock.patch.object(bundle, "require_build_host"):
            with self.assertRaisesRegex(bundle.BundleError, "unpackaged"):
                bundle.build(root, binary_dir, output, commit, RUN_URL)
        self.assertFalse(output.exists())

    @POSIX_ONLY
    def test_cli_verify_extract_and_postflight(self):
        archive = self.base / "bundle.tar.gz"
        archive.write_bytes(self.archive)
        destination = self.base / "终端 解压"
        with mock.patch("sys.stdout", new_callable=io.StringIO) as output:
            status = bundle.main(["verify", str(archive), "--source-commit", COMMIT,
                                  "--ci-run-url", RUN_URL, "--extract-to", str(destination),
                                  "--verify-extracted", str(destination)])
        self.assertEqual(status, 0)
        result = json.loads(output.getvalue())
        self.assertEqual(result["verified_extracted"], str(destination))
        self.assertEqual(result["file_count"], len(self.payload) + 1)
        with mock.patch("sys.stderr", new_callable=io.StringIO):
            self.assertEqual(bundle.main(["verify", str(archive), "--source-commit", "b" * 40]), 1)


if __name__ == "__main__":
    unittest.main()
