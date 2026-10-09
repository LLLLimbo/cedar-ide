#!/usr/bin/env python3
"""Build or verify the bounded, unsigned Windows x64 development bundle.

Requires Python 3.11+. This packages existing normal release executables; it
does not compile, sign, publish, prove binary provenance, or authenticate a CI
run. The manifest records the supplied source/CI mapping and payload hashes.
Verification detects corruption and inventory changes, not a forged manifest.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import subprocess
import sys
import tomllib
import zipfile
import zlib


REPOSITORY = "https://github.com/LLLLimbo/cedar-ide"
TARGET = "x86_64-pc-windows-msvc"
MANIFEST = "BUNDLE_MANIFEST.json"
BINARIES = ("cedar.exe", "cedar-agent.exe")
SOURCE_FILES = {
    "LICENSE-MIT": "LICENSE-MIT",
    "LICENSE-APACHE": "LICENSE-APACHE",
    "THIRD_PARTY_NOTICES.md": "THIRD_PARTY_NOTICES.md",
    "docs/WINDOWS_QUICKSTART.zh-CN.md": "WINDOWS_QUICKSTART.zh-CN.md",
    "docs/WINDOWS_JAVA_SETUP.md": "WINDOWS_JAVA_SETUP.md",
    "docs/GIT_VIEWS.md": "GIT_VIEWS.md",
    "docs/JAVA_IMPORTS.md": "JAVA_IMPORTS.md",
    "docs/BUILD_PROBLEMS.md": "BUILD_PROBLEMS.md",
    "docs/MAVEN_PROJECTS.md": "MAVEN_PROJECTS.md",
    "docs/TEST_RESULTS.md": "TEST_RESULTS.md",
    "docs/JAVA_TYPE_SEARCH.md": "JAVA_TYPE_SEARCH.md",
    "docs/IDLE_DISCONNECT.md": "IDLE_DISCONNECT.md",
    "docs/DRAFT_MERGE.md": "DRAFT_MERGE.md",
}
# Exact notice names currently collected by collect_licenses.py. Adding a new
# upstream notice deliberately requires reviewing this allowlist.
NOTICE_NAMES = frozenset({
    "AUTHORS", "COPYING", "LICENSE", "LICENSE-0BSD", "LICENSE-APACHE",
    "LICENSE-APACHE.md", "LICENSE-APACHE.txt", "LICENSE-Apache",
    "LICENSE-Apache-2.0_WITH_LLVM-exception", "LICENSE-BSD", "LICENSE-LIBM-MIT",
    "LICENSE-MIT", "LICENSE-MIT.md", "LICENSE-MIT.txt", "LICENSE-THIRD-PARTY",
    "LICENSE-UNICODE", "LICENSE-ZLIB", "LICENSE-ZLIB.md", "LICENSE.APACHE",
    "LICENSE.MIT", "LICENSE.md", "LICENSE.txt", "NOTICES.md",
    "license-apache-2.0", "license-mit",
})
FONT_NOTICE_NAMES = frozenset({
    "Hack-Regular.txt", "OFL.txt", "UFL.txt", "emoji-icon-font-mit-license.txt",
})
FIXED_DATE = (1980, 1, 1, 0, 0, 0)
FILE_ATTRIBUTES = (stat.S_IFREG | 0o644) << 16
MAX_BINARY_BYTES = 128 * 1024 * 1024
MAX_TEXT_BYTES = 2 * 1024 * 1024
MAX_TOTAL_BYTES = 300 * 1024 * 1024
MAX_ARCHIVE_BYTES = 304 * 1024 * 1024
MAX_FILES = 2048
MAX_GIT_BYTES = 8 * 1024 * 1024
HEX_COMMIT = re.compile(r"[0-9a-f]{40}\Z")
HEX_HASH = re.compile(r"[0-9a-f]{64}\Z")
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?\Z")
RUN_URL = re.compile(re.escape(REPOSITORY) + r"/actions/runs/([1-9][0-9]{0,19})\Z")
COMPONENT = re.compile(r"[A-Za-z0-9][A-Za-z0-9._+-]*\Z")
DEVICE = re.compile(r"(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)", re.I)


class BundleError(ValueError):
    """An input violates the bundle contract."""


def require(condition, message):
    if not condition:
        raise BundleError(message)


def safe_name(name):
    require(isinstance(name, str) and 0 < len(name) <= 220,
            "Invalid archive path length or type")
    for component in name.split("/"):
        require(COMPONENT.fullmatch(component) is not None
                and not component.endswith(".") and not DEVICE.match(component),
                f"Unsafe archive path: {name!r}")
    return name


def is_notice(name):
    parts = name.split("/")
    if len(parts) != 3 or parts[0] != "third-party-licenses":
        return False
    return parts[2] in NOTICE_NAMES or (
        parts[1].startswith("epaint_default_fonts-")
        and parts[2] in FONT_NOTICE_NAMES)


def allowed_payload(name):
    safe_name(name)
    return name in BINARIES or name in SOURCE_FILES.values() or is_notice(name)


def byte_limit(name):
    return MAX_BINARY_BYTES if name in BINARIES else MAX_TEXT_BYTES


def is_reparse(info):
    return bool(getattr(info, "st_file_attributes", 0) & 0x400)


def identity(info, cross_api=False):
    # Windows pathname stat invents execute bits for .exe, while descriptor
    # stat does not. ctime semantics also differ between APIs/Python versions.
    common = (info.st_dev, info.st_ino, stat.S_IFMT(info.st_mode), info.st_size, info.st_mtime_ns)
    return common if cross_api else common + (info.st_mode, info.st_ctime_ns)


def checked_path(path, leaf_missing=False):
    """Check every component without resolving away symlinks or junctions."""
    path = Path(os.path.abspath(path))
    components = list(reversed(path.parents)) + [path]
    for component in components:
        try:
            info = component.lstat()
        except FileNotFoundError:
            require(leaf_missing and component == path,
                    f"Missing parent or input: {component}")
            return path
        require(not stat.S_ISLNK(info.st_mode) and not is_reparse(info),
                f"Symlink or reparse point is not allowed: {component}")
        if component != path:
            require(stat.S_ISDIR(info.st_mode), f"Not a directory: {component}")
    return path


def read_regular(path, limit):
    """Read a bounded regular file, checking identity and metadata throughout."""
    path = checked_path(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode), f"Not a regular file: {path}")
    require(0 <= before.st_size <= limit, f"File exceeds size limit: {path}")
    # Nonblocking open prevents a raced-in FIFO from hanging before fstat can
    # reject it. It has no effect on ordinary file reads.
    flags = (os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
             | getattr(os, "O_NONBLOCK", 0))
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor, "rb") as source:
        opened = os.fstat(source.fileno())
        require(identity(before, cross_api=True) == identity(opened, cross_api=True)
                and not is_reparse(opened),
                f"File changed while opening: {path}")
        data = source.read(limit + 1)
        after = os.fstat(source.fileno())
    checked_path(path)
    require(identity(opened) == identity(after) and identity(before) == identity(path.lstat())
            and len(data) == before.st_size and len(data) <= limit,
            f"File changed while reading or exceeds limit: {path}")
    return data


def validate_pe(data, name):
    require(len(data) >= 64 and data[:2] == b"MZ", f"Not a PE executable: {name}")
    offset = struct.unpack_from("<I", data, 0x3C)[0]
    require(64 <= offset <= min(len(data) - 24, 1024 * 1024),
            f"Invalid PE header offset: {name}")
    require(data[offset:offset + 4] == b"PE\0\0", f"Missing PE signature: {name}")
    machine, sections = struct.unpack_from("<HH", data, offset + 4)
    optional_size, characteristics = struct.unpack_from("<HH", data, offset + 20)
    require(machine == 0x8664 and 1 <= sections <= 96,
            f"Expected an x64 PE executable: {name}")
    require(characteristics & 0x0002 and not characteristics & 0x2000,
            f"Expected an executable, not a DLL: {name}")
    optional = offset + 24
    require(optional_size >= 112 and optional + optional_size + sections * 40 <= len(data),
            f"Truncated PE headers: {name}")
    require(struct.unpack_from("<H", data, optional)[0] == 0x20B,
            f"Expected PE32+ executable: {name}")
    require(struct.unpack_from("<H", data, optional + 68)[0] in (2, 3),
            f"Expected a Windows GUI or console executable: {name}")
    for index in range(sections):
        section = optional + optional_size + index * 40
        raw_size, raw_offset = struct.unpack_from("<II", data, section + 16)
        require(raw_size == 0 or (raw_offset >= section + 40
                                 and raw_offset + raw_size <= len(data)),
                f"Truncated PE section: {name}")


def git(root, *arguments):
    result = subprocess.run(["git", "-C", str(root), *arguments],
                            check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=30)
    require(len(result.stdout) <= MAX_GIT_BYTES, "Git output exceeds limit")
    return result.stdout


def clean_commit(root, expected):
    require(isinstance(expected, str) and HEX_COMMIT.fullmatch(expected),
            "Source commit must be a full lowercase 40-character SHA")
    actual = git(root, "rev-parse", "HEAD").decode("ascii").strip()
    require(actual == expected, "Supplied source commit does not match checkout HEAD")
    require(not git(root, "status", "--porcelain=v1", "--untracked-files=all"),
            "Source checkout must be clean, including non-ignored untracked files")


def tracked_sources(root):
    result = {}
    for record in git(root, "ls-tree", "-r", "-z", "HEAD").split(b"\0"):
        if not record:
            continue
        header, raw_name = record.split(b"\t", 1)
        name = raw_name.decode("utf-8")
        if name not in SOURCE_FILES and name != "Cargo.toml" and not name.startswith("third-party-licenses/"):
            continue
        mode, kind, object_id = header.decode("ascii").split(" ")
        require(mode in ("100644", "100755") and kind == "blob",
                f"Source input must be a tracked regular file: {name}")
        if name.startswith("third-party-licenses/"):
            safe_name(name)
            require(is_notice(name), f"Unapproved third-party notice path: {name}")
        result[name] = object_id
    require(set(SOURCE_FILES) | {"Cargo.toml"} <= result.keys(),
            "Required bundle documentation or licenses are not committed")
    require(any(is_notice(name) for name in result), "No committed third-party license notices")
    require(len(result) + len(BINARIES) <= MAX_FILES, "Too many bundle files")
    return result


def blob_hash(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest()


def source_bytes(root, relative, object_id):
    data = read_regular(root / relative, MAX_TEXT_BYTES)
    # Git for Windows may check text out with CRLF. Package canonical LF bytes
    # only when those bytes exactly reproduce the committed Git blob identity.
    if blob_hash(data) != object_id:
        data = data.replace(b"\r\n", b"\n")
    require(blob_hash(data) == object_id, f"Source file differs from HEAD: {relative}")
    return data


def canonical_json(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode("utf-8")


def file_records(payload):
    return [{"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
            for name, data in sorted(payload.items())]


def make_manifest(version, commit, ci_run_url, payload):
    return {
        "schema_version": 1, "product": "cedar", "bundle_kind": "windows-development",
        "version": version, "target": TARGET, "build_profile": "release",
        "unsigned": True,
        "source": {"repository": REPOSITORY, "commit": commit,
                   "commit_url": f"{REPOSITORY}/commit/{commit}"},
        "ci": {"run_url": ci_run_url, "workflow": ".github/workflows/ci.yml"},
        "files": file_records(payload),
    }


def zip_info(name):
    info = zipfile.ZipInfo(name, FIXED_DATE)
    info.create_system = 3
    info.external_attr = FILE_ATTRIBUTES
    info.compress_type = zipfile.ZIP_DEFLATED
    return info


def archive_bytes(payload, manifest):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED,
                         compresslevel=6, allowZip64=False) as archive:
        for name, data in sorted({**payload, MANIFEST: canonical_json(manifest)}.items()):
            archive.writestr(zip_info(name), data, compresslevel=6)
    data = buffer.getvalue()
    require(len(data) <= MAX_ARCHIVE_BYTES, "Archive exceeds size limit")
    return data


def exact_keys(value, keys, description):
    require(isinstance(value, dict) and set(value) == set(keys),
            f"Invalid {description} fields")


def no_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def validate_manifest(data, expected_commit=None, expected_ci=None):
    try:
        manifest = json.loads(data.decode("utf-8"), object_pairs_hook=no_duplicate_keys)
    except (UnicodeError, json.JSONDecodeError, RecursionError) as error:
        raise BundleError("Manifest is not valid bounded UTF-8 JSON") from error
    exact_keys(manifest, ("schema_version", "product", "bundle_kind", "version", "target",
                          "build_profile", "unsigned", "source", "ci", "files"), "manifest")
    require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1
            and manifest["product"] == "cedar" and manifest["bundle_kind"] == "windows-development"
            and manifest["target"] == TARGET and manifest["build_profile"] == "release"
            and manifest["unsigned"] is True, "Unsupported bundle format or platform")
    require(isinstance(manifest["version"], str) and VERSION.fullmatch(manifest["version"]),
            "Invalid bundle version")
    source = manifest["source"]
    exact_keys(source, ("repository", "commit", "commit_url"), "source")
    commit = source["commit"]
    require(isinstance(commit, str) and HEX_COMMIT.fullmatch(commit)
            and source["repository"] == REPOSITORY
            and source["commit_url"] == f"{REPOSITORY}/commit/{commit}", "Invalid source mapping")
    exact_keys(manifest["ci"], ("run_url", "workflow"), "CI")
    ci_url = manifest["ci"]["run_url"]
    require(isinstance(ci_url, str) and RUN_URL.fullmatch(ci_url)
            and manifest["ci"]["workflow"] == ".github/workflows/ci.yml", "Invalid CI mapping")
    require(expected_commit is None or commit == expected_commit, "Archive source commit mismatch")
    require(expected_ci is None or ci_url == expected_ci, "Archive CI run URL mismatch")
    files = manifest["files"]
    require(isinstance(files, list) and 1 <= len(files) < MAX_FILES, "Invalid manifest file count")
    records = {}
    case_names = set()
    total = 0
    for record in files:
        exact_keys(record, ("path", "bytes", "sha256"), "file record")
        name = safe_name(record["path"])
        require(allowed_payload(name), f"File outside bundle allowlist: {name}")
        require(name.casefold() not in case_names, f"Duplicate or case-colliding path: {name}")
        case_names.add(name.casefold())
        require(type(record["bytes"]) is int and 0 < record["bytes"] <= byte_limit(name),
                f"Invalid file size: {name}")
        require(isinstance(record["sha256"], str) and HEX_HASH.fullmatch(record["sha256"]),
                f"Invalid SHA256: {name}")
        records[name] = record
        total += record["bytes"]
    require(total <= MAX_TOTAL_BYTES, "Payload exceeds total size limit")
    require(list(records) == sorted(records), "Manifest file inventory is not sorted")
    require(set(BINARIES) | set(SOURCE_FILES.values()) <= records.keys(),
            "Manifest omits a required payload file")
    require(any(is_notice(name) for name in records), "Manifest has no third-party license notices")
    require(data == canonical_json(manifest), "Manifest is not in canonical format")
    return manifest, records


def inflate_entry(data, entry):
    start = entry.header_offset + 30 + len(entry.filename.encode("ascii"))
    compressed = memoryview(data)[start:start + entry.compress_size]
    inflater = zlib.decompressobj(-15)
    content = inflater.decompress(compressed, entry.file_size + 1)
    require(len(content) == entry.file_size and inflater.eof
            and not inflater.unused_data and not inflater.unconsumed_tail,
            f"Incomplete, oversized, or hidden data in DEFLATE stream: {entry.filename}")
    require(zlib.crc32(content) & 0xFFFFFFFF == entry.CRC,
            f"ZIP checksum mismatch: {entry.filename}")
    return content


def verify_bytes(data, expected_commit=None, expected_ci=None):
    require(len(data) <= MAX_ARCHIVE_BYTES, "Archive exceeds size limit")
    require(len(data) >= 22, "Truncated ZIP end record")
    signature, disk, central_disk, disk_count, count, central_size, central_offset, comment_size = (
        struct.unpack_from("<4s4H2IH", data, len(data) - 22))
    # Bound the central directory before ZipFile allocates objects for it. ZIP64,
    # split archives and comments are deliberately outside this small format.
    require(signature == b"PK\x05\x06" and disk == central_disk == comment_size == 0
            and disk_count == count and 0 < count <= MAX_FILES
            and 46 * count <= central_size <= (46 + 220) * count
            and central_offset + central_size + 22 == len(data),
            "Invalid or oversized ZIP central directory")
    payload = {}
    try:
        with zipfile.ZipFile(io.BytesIO(data), "r") as archive:
            entries = archive.infolist()
            require(len(entries) == count, "Invalid archive file count")
            require(not archive.comment, "Archive comments are not allowed")
            names, case_names = [], set()
            total = 0
            next_offset = 0
            central_records = []
            for entry in entries:
                name = safe_name(entry.filename)
                require(entry.orig_filename == name, "Truncated or NUL-containing archive path")
                require(name.casefold() not in case_names, f"Duplicate or case-colliding archive path: {name}")
                names.append(name)
                case_names.add(name.casefold())
                require(name == MANIFEST or allowed_payload(name), f"Unexpected archive file: {name}")
                require(entry.create_system == 3 and entry.external_attr == FILE_ATTRIBUTES
                        and entry.compress_type == zipfile.ZIP_DEFLATED and entry.flag_bits == 0
                        and entry.date_time == FIXED_DATE and not entry.extra and not entry.comment,
                        f"Unsafe or noncanonical ZIP metadata: {name}")
                require(0 < entry.file_size <= byte_limit(name)
                        and 0 < entry.compress_size <= byte_limit(name) + 65536,
                        f"Archive member exceeds size limit: {name}")
                require(entry.header_offset == next_offset, "Unexpected data or ZIP entry ordering")
                # Validate both headers, including bytes zipfile otherwise ignores.
                # This permits different zlib versions without admitting hidden data.
                encoded_name = name.encode("ascii")
                local_header = struct.pack("<4s5H3I2H", b"PK\x03\x04", 20, 0, 8, 0, 33,
                                           entry.CRC, entry.compress_size, entry.file_size,
                                           len(encoded_name), 0) + encoded_name
                require(data[next_offset:next_offset + len(local_header)] == local_header,
                        f"Invalid local ZIP header: {name}")
                central_records.append(struct.pack(
                    "<4s6H3I5H2I", b"PK\x01\x02", (3 << 8) | 20, 20, 0, 8, 0, 33,
                    entry.CRC, entry.compress_size, entry.file_size,
                    len(encoded_name), 0, 0, 0, 0, FILE_ATTRIBUTES, next_offset) + encoded_name)
                next_offset += len(local_header) + entry.compress_size
                total += entry.file_size
                require(total <= MAX_TOTAL_BYTES + MAX_TEXT_BYTES, "Archive expansion exceeds limit")
            require(names == sorted(names) and MANIFEST in names, "Missing manifest or unsorted archive")
            central = b"".join(central_records)
            end = struct.pack("<4s4H2IH", b"PK\x05\x06", 0, 0, len(entries), len(entries),
                              len(central), next_offset, 0)
            require(data[next_offset:] == central + end, "ZIP has unaccounted or noncanonical data")
            manifest_data = inflate_entry(data, archive.getinfo(MANIFEST))
            manifest, records = validate_manifest(manifest_data, expected_commit, expected_ci)
            require(set(names) == set(records) | {MANIFEST}, "Archive inventory differs from manifest")
            for entry in entries:
                if entry.filename == MANIFEST:
                    continue
                record = records[entry.filename]
                require(entry.file_size == record["bytes"], f"Size mismatch: {entry.filename}")
                content = inflate_entry(data, entry)
                require(hashlib.sha256(content).hexdigest() == record["sha256"],
                        f"SHA256 mismatch: {entry.filename}")
                if entry.filename in BINARIES:
                    validate_pe(content, entry.filename)
                payload[entry.filename] = content
    except (zipfile.BadZipFile, EOFError, struct.error, NotImplementedError, zlib.error) as error:
        raise BundleError(f"Invalid ZIP: {error}") from error
    return manifest, {**payload, MANIFEST: manifest_data}


def create_file(path, data):
    path = checked_path(path, leaf_missing=True)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags, 0o644)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
    except BaseException:
        path.unlink()
        raise


def extract_payload(payload, destination):
    destination = checked_path(destination, leaf_missing=True)
    require(not destination.exists(), f"Extraction destination already exists: {destination}")
    destination.mkdir(mode=0o700)
    try:
        for name, data in sorted(payload.items()):
            safe_name(name)
            target = destination.joinpath(*name.split("/"))
            relative_parent = target.parent.relative_to(destination)
            parent = destination
            for part in relative_parent.parts:
                parent = parent / part
                checked_path(parent, leaf_missing=True)
                if not parent.exists():
                    parent.mkdir(mode=0o700)
            create_file(target, data)
        for name, data in payload.items():
            require(read_regular(destination.joinpath(*name.split("/")), byte_limit(name)) == data,
                    f"Extracted content changed: {name}")
    except BaseException:
        # This directory was created exclusively by this invocation.
        shutil.rmtree(destination)
        raise
    return destination


def receipt(path, data, manifest):
    return {"path": str(path), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
            "version": manifest["version"], "source_commit": manifest["source"]["commit"],
            "ci_run_url": manifest["ci"]["run_url"], "file_count": len(manifest["files"]) + 1}


def build(root, binary_dir, output, source_commit, ci_run_url):
    root = checked_path(root)
    require(isinstance(ci_run_url, str) and RUN_URL.fullmatch(ci_run_url),
            f"CI run URL must be {REPOSITORY}/actions/runs/<positive run id>")
    clean_commit(root, source_commit)
    sources = tracked_sources(root)
    cargo = tomllib.loads(source_bytes(root, "Cargo.toml", sources["Cargo.toml"]).decode("utf-8"))
    version = cargo.get("workspace", {}).get("package", {}).get("version")
    require(isinstance(version, str) and VERSION.fullmatch(version), "Invalid Cargo workspace version")
    payload = {}
    for relative, object_id in sorted(sources.items()):
        if relative != "Cargo.toml":
            payload[SOURCE_FILES.get(relative, relative)] = source_bytes(root, relative, object_id)
    for name in BINARIES:
        content = read_regular(Path(binary_dir) / name, MAX_BINARY_BYTES)
        validate_pe(content, name)
        payload[name] = content
    require(sum(map(len, payload.values())) <= MAX_TOTAL_BYTES, "Payload exceeds size limit")
    manifest = make_manifest(version, source_commit, ci_run_url, payload)
    validate_manifest(canonical_json(manifest))
    clean_commit(root, source_commit)
    data = archive_bytes(payload, manifest)
    verify_bytes(data, source_commit, ci_run_url)
    output = checked_path(output, leaf_missing=True)
    create_file(output, data)
    require(read_regular(output, MAX_ARCHIVE_BYTES) == data, "Written archive changed")
    return receipt(output, data, manifest)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build", help="Package existing normal release binaries")
    build_parser.add_argument("--source-commit", required=True)
    build_parser.add_argument("--ci-run-url", required=True)
    build_parser.add_argument("--output", required=True, type=Path)
    build_parser.add_argument("--binary-dir", type=Path, default=None)
    verify_parser = commands.add_parser("verify", help="Verify every entry; optionally extract into a new directory")
    verify_parser.add_argument("archive", type=Path)
    verify_parser.add_argument("--extract-to", type=Path)
    verify_parser.add_argument("--source-commit")
    verify_parser.add_argument("--ci-run-url")
    args = parser.parse_args(argv)
    try:
        if args.command == "build":
            root = Path(__file__).absolute().parent.parent
            result = build(root, args.binary_dir or root / "target/release", args.output,
                           args.source_commit, args.ci_run_url)
        else:
            data = read_regular(args.archive, MAX_ARCHIVE_BYTES)
            manifest, payload = verify_bytes(data, args.source_commit, args.ci_run_url)
            result = receipt(args.archive.absolute(), data, manifest)
            if args.extract_to is not None:
                result["extracted_to"] = str(extract_payload(payload, args.extract_to))
        print(json.dumps(result, indent=2))
    except (BundleError, OSError, subprocess.SubprocessError, tomllib.TOMLDecodeError, UnicodeError) as error:
        print(f"Bundle error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
