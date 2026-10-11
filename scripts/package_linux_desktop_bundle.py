#!/usr/bin/env python3
"""Package/verify the unsigned Ubuntu 24.04 amd64 matched Cedar desktop bundle.

Python 3.11+, standard library only. Never builds or executes a binary. ELF ABI
requirements are measured from the packaged bytes, not the packaging host.
Hashes and source/CI links establish integrity, not authenticated provenance or
proof of compiler flags. The CI build is responsible for normal default features.
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
import tarfile
import tomllib
import zlib

import package_linux_agent_bundle as agent_bundle
from package_linux_agent_bundle import (
    BASELINE, GZIP_HEADER, INTERPRETER, TARGET, create_file, gzip_bytes, require_build_host,
)

# Reuse local-file/Git/notice boundaries and pure Linux output primitives.
# Keep the agent-only archive and ELF contracts unchanged.
from package_windows_bundle import (
    BundleError, COMPONENT, HEX_COMMIT, HEX_HASH, MAX_BINARY_BYTES, MAX_FILES,
    MAX_TEXT_BYTES, REPOSITORY, RUN_URL, VERSION, blob_hash, canonical_json,
    checked_path, clean_commit, exact_keys, git, is_notice, no_duplicate_keys,
    read_regular, require, safe_name, source_bytes,
)

MANIFEST = "BUNDLE_MANIFEST.json"
DESKTOP = "cedar"
AGENT = "cedar-agent"
BINARIES = (DESKTOP, AGENT)
GUIDE = "LINUX_DESKTOP_QUICKSTART.zh-CN.md"
SOURCE_FILES = {
    "LICENSE-MIT": "LICENSE-MIT",
    "LICENSE-APACHE": "LICENSE-APACHE",
    "THIRD_PARTY_NOTICES.md": "THIRD_PARTY_NOTICES.md",
    "docs/" + GUIDE: GUIDE,
    "docs/MAVEN_PROJECTS.md": "MAVEN_PROJECTS.md",
    "docs/MAVEN_DEPENDENCIES.md": "MAVEN_DEPENDENCIES.md",
    "docs/SAVE_ALL.md": "SAVE_ALL.md",
    "docs/EXPLORER_TREE.md": "EXPLORER_TREE.md",
}
MAX_TOTAL_BYTES = 300 * 1024 * 1024
MAX_TAR_BYTES = MAX_TOTAL_BYTES + MAX_FILES * 1024 + MAX_TEXT_BYTES
MAX_ARCHIVE_BYTES = MAX_TAR_BYTES + 65536
# Direct dependencies measured in the normal GUI ELF. X11/Wayland/OpenGL
# libraries loaded at runtime are separate requirements, documented in GUIDE.
DESKTOP_SYSTEM_LIBRARIES = agent_bundle.SYSTEM_LIBRARIES | {"libm.so.6"}


def allowed_payload(name):
    safe_name(name)
    return name in BINARIES or name in SOURCE_FILES.values() or is_notice(name)


def byte_limit(name):
    return MAX_BINARY_BYTES if name in BINARIES else MAX_TEXT_BYTES


def file_mode(name):
    return 0o755 if name in BINARIES else 0o644


def inspect_elf(data, name=DESKTOP):
    """Read only ELF64 program/dynamic headers used by the Linux loader.

    Section names, strings outside the loader's tables, ldd and host libraries
    cannot establish binary requirements and are intentionally unused.
    """
    require(name in BINARIES, "Unknown desktop bundle executable")
    if name == AGENT:
        return agent_bundle.inspect_elf(data)
    # The GUI uses the same bounded parser as the agent. This local copy keeps
    # its reviewed libm dependency from broadening the agent-only contract.
    require(64 <= len(data) <= MAX_BINARY_BYTES, "Invalid ELF size")
    require(data[:7] == b"\x7fELF\x02\x01\x01" and data[7] in (0, 3)
            and data[8:16] == bytes(8), "Expected ELF64 little-endian Linux/System V executable")
    kind, machine, version, entry, phoff, _, flags, ehsize, phsize, phnum, _, _, _ = (
        struct.unpack_from("<HHIQQQIHHHHHH", data, 16))
    require(kind in (2, 3) and machine == 62 and version == 1 and flags == 0
            and ehsize == 64 and phsize == 56 and 1 <= phnum <= 64
            and 64 <= phoff <= len(data) - phnum * 56,
            "Expected bounded x86-64 ELF executable headers")
    loads, dynamics, interpreters = [], [], []
    for index in range(phnum):
        ptype, pflags, offset, address, _, size, memory_size, align = struct.unpack_from(
            "<IIQQQQQQ", data, phoff + index * 56)
        require(offset <= len(data) and size <= len(data) - offset,
                "ELF segment outside file")
        if ptype == 1:
            require(size <= memory_size and address + memory_size < 2 ** 64
                    and (align in (0, 1) or (align & (align - 1) == 0
                         and offset % align == address % align)), "Invalid ELF load segment")
            loads.append((address, offset, size, memory_size, pflags))
        elif ptype == 2:
            require(size == memory_size, "ELF dynamic table must be entirely file-backed")
            dynamics.append((offset, size, address))
        elif ptype == 3:
            interpreters.append(data[offset:offset + size])
    require(loads and len(dynamics) == len(interpreters) == 1
            and interpreters[0] == INTERPRETER.encode("ascii") + b"\0"
            and any(flags & 1 and address <= entry < address + size
                    for address, _, size, _, flags in loads),
            "Expected one GNU/Linux interpreter, dynamic table and executable entry point")
    ordered_loads = sorted(loads)
    require(all(left[0] + left[3] <= right[0] for left, right in zip(ordered_loads, ordered_loads[1:])),
            "Overlapping or ambiguous ELF load memory ranges")

    def mapped(address, size):
        matches = [offset + address - start for start, offset, length, _, _ in loads
                   if start <= address and 0 <= size <= length
                   and address - start <= length - size]
        require(len(matches) == 1, "Unmapped or ambiguous ELF dynamic address")
        return matches[0]

    dynamic_offset, dynamic_size, dynamic_address = dynamics[0]
    require(0 < dynamic_size <= 65536 and dynamic_size % 16 == 0,
            "Invalid ELF dynamic table size")
    require(mapped(dynamic_address, dynamic_size) == dynamic_offset,
            "ELF dynamic table differs from its load mapping")
    tags, needed_offsets, ended = {}, [], False
    for offset in range(dynamic_offset, dynamic_offset + dynamic_size, 16):
        tag, value = struct.unpack_from("<qQ", data, offset)
        if ended or tag == 0:
            require(tag == value == 0, "Hidden entries after ELF DT_NULL")
            ended = True
            continue
        require(tag not in (15, 29, 0x6ffffefb, 0x6ffffefc, 0x7ffffffd, 0x7fffffff),
                "ELF custom search paths, audit or filter libraries are not supported")
        if tag == 1:
            needed_offsets.append(value)
        else:
            require(tag not in tags, "Duplicate ELF dynamic tag")
            tags[tag] = value
    require(ended and 1 <= len(needed_offsets) <= 64
            and {5, 10, 0x6ffffffe, 0x6fffffff} <= tags.keys(),
            "Missing ELF dynamic strings, dependencies or version requirements")
    string_size = tags[10]
    require(1 <= string_size <= 1024 * 1024, "ELF string table exceeds limit")
    string_offset = mapped(tags[5], string_size)
    strings = data[string_offset:string_offset + string_size]

    def string(index):
        require(0 <= index < len(strings), "Invalid ELF string offset")
        end = strings.find(b"\0", index, min(index + 256, len(strings)))
        require(end > index, "Invalid or oversized ELF dynamic string")
        try:
            value = strings[index:end].decode("ascii")
        except UnicodeDecodeError as error:
            raise BundleError("Non-ASCII ELF dynamic string") from error
        require(COMPONENT.fullmatch(value) is not None, "Unsafe ELF dependency/version name")
        return value

    needed = [string(index) for index in needed_offsets]
    require(len(set(needed)) == len(needed) and "libc.so.6" in needed,
            "Duplicate dependencies or missing GNU libc")
    require(set(needed) <= DESKTOP_SYSTEM_LIBRARIES, "Unexpected ELF dependency outside reviewed system libraries")
    count = tags[0x6fffffff]
    require(1 <= count <= 64, "Invalid ELF version requirement count")
    cursor, requirements, libraries, glibc = tags[0x6ffffffe], [], set(), set()
    for index in range(count):
        record = mapped(cursor, 16)
        ver, auxiliary_count, library_offset, auxiliary, next_record = struct.unpack_from(
            "<HHIII", data, record)
        library = string(library_offset)
        require(ver == 1 and 1 <= auxiliary_count <= 256 and auxiliary >= 16
                and library in needed and library not in libraries,
                "Invalid ELF version requirement record")
        libraries.add(library)
        auxiliary_cursor, versions = cursor + auxiliary, set()
        for item in range(auxiliary_count):
            auxiliary_record = mapped(auxiliary_cursor, 16)
            _, flags, _, name_offset, next_auxiliary = struct.unpack_from(
                "<IHHII", data, auxiliary_record)
            name = string(name_offset)
            require(flags in (0, 2) and name not in versions, "Invalid ELF version auxiliary record")
            versions.add(name)
            if name.startswith("GLIBC_"):
                number = name.removeprefix("GLIBC_")
                require(re.fullmatch(r"[0-9]+\.[0-9]+(?:\.[0-9]+)?", number) is not None,
                        "Unsupported nonnumeric GLIBC requirement")
                glibc.add(number)
            require((next_auxiliary == 0) if item == auxiliary_count - 1 else (next_auxiliary >= 16),
                    "Invalid ELF version auxiliary chain")
            auxiliary_cursor += next_auxiliary
        requirements.append({"library": library, "versions": sorted(versions)})
        require((next_record == 0) if index == count - 1 else (next_record >= 16),
                "Invalid ELF version requirement chain")
        cursor += next_record
    require(glibc, "No measured GLIBC requirements")
    required_glibc = max(glibc, key=lambda value: tuple(map(int, value.split("."))))
    require(tuple(map(int, required_glibc.split("."))) <= (2, 39),
            "ELF requires GLIBC newer than Ubuntu 24.04 baseline (2.39)")
    return {"format": "ELF64", "endianness": "little", "machine": "EM_X86_64",
            "interpreter": INTERPRETER, "needed": sorted(needed),
            "version_requirements": sorted(requirements, key=lambda item: item["library"]),
            "required_glibc": required_glibc}


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
        require(mode == "100644" and kind == "blob", f"Source must be a tracked non-executable file: {name}")
        if name.startswith("third-party-licenses/"):
            safe_name(name)
            require(is_notice(name), f"Unapproved third-party notice path: {name}")
        result[name] = object_id
    require(set(SOURCE_FILES) | {"Cargo.toml"} <= result.keys(), "Required source documents are not committed")
    require(any(is_notice(name) for name in result), "No committed third-party notices")
    require(len(result) + len(BINARIES) <= MAX_FILES, "Too many source files")
    return result


def source_path(name):
    return next((source for source, target in SOURCE_FILES.items() if target == name), name)


def make_manifest(version, commit, ci_run_url, payload):
    files = []
    for name, data in sorted(payload.items()):
        source = None
        if name not in BINARIES:
            relative = source_path(name)
            source = {"path": relative, "git_blob": blob_hash(data),
                      "blob_url": f"{REPOSITORY}/blob/{commit}/{relative}"}
        files.append({"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                      "mode": format(file_mode(name), "04o"), "source": source})
    return {"schema_version": 1, "product": "cedar", "bundle_kind": "linux-desktop-development",
            "version": version, "target": TARGET, "baseline": dict(BASELINE),
            "build_profile": "release", "features": "default", "unsigned": True,
            "source": {"repository": REPOSITORY, "commit": commit,
                       "commit_url": f"{REPOSITORY}/commit/{commit}"},
            "ci": {"run_url": ci_run_url, "workflow": ".github/workflows/ci.yml"},
            "abi": {name: inspect_elf(payload[name], name) for name in BINARIES}, "files": files}


def validate_manifest(data, expected_commit=None, expected_ci=None):
    try:
        manifest = json.loads(data.decode("utf-8"), object_pairs_hook=no_duplicate_keys)
    except (UnicodeError, json.JSONDecodeError, RecursionError) as error:
        raise BundleError("Manifest is not valid bounded UTF-8 JSON") from error
    exact_keys(manifest, ("schema_version", "product", "bundle_kind", "version", "target", "baseline",
                          "build_profile", "features", "unsigned", "source", "ci", "abi", "files"), "manifest")
    require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1
            and manifest["product"] == "cedar" and manifest["bundle_kind"] == "linux-desktop-development"
            and manifest["target"] == TARGET and manifest["baseline"] == BASELINE
            and manifest["build_profile"] == "release" and manifest["features"] == "default"
            and manifest["unsigned"] is True, "Unsupported desktop bundle format or platform")
    require(isinstance(manifest["version"], str) and VERSION.fullmatch(manifest["version"]), "Invalid bundle version")
    source = manifest["source"]
    exact_keys(source, ("repository", "commit", "commit_url"), "source")
    commit = source["commit"]
    require(isinstance(commit, str) and HEX_COMMIT.fullmatch(commit) and source["repository"] == REPOSITORY
            and source["commit_url"] == f"{REPOSITORY}/commit/{commit}", "Invalid source mapping")
    exact_keys(manifest["ci"], ("run_url", "workflow"), "CI")
    ci_url = manifest["ci"]["run_url"]
    require(isinstance(ci_url, str) and RUN_URL.fullmatch(ci_url)
            and manifest["ci"]["workflow"] == ".github/workflows/ci.yml", "Invalid CI mapping")
    require(expected_commit is None or commit == expected_commit, "Archive source commit mismatch")
    require(expected_ci is None or ci_url == expected_ci, "Archive CI run URL mismatch")
    exact_keys(manifest["abi"], BINARIES, "executable ABI inventory")
    files = manifest["files"]
    require(isinstance(files, list) and 1 <= len(files) < MAX_FILES, "Invalid manifest file count")
    records, names, total = {}, set(), 0
    for record in files:
        exact_keys(record, ("path", "bytes", "sha256", "mode", "source"), "file record")
        name = safe_name(record["path"])
        require(allowed_payload(name), f"File outside bundle allowlist: {name}")
        require(name.casefold() not in names, f"Duplicate or case-colliding path: {name}")
        names.add(name.casefold())
        require(type(record["bytes"]) is int and 0 < record["bytes"] <= byte_limit(name), f"Invalid file size: {name}")
        require(isinstance(record["sha256"], str) and HEX_HASH.fullmatch(record["sha256"]), f"Invalid SHA256: {name}")
        require(record["mode"] == format(file_mode(name), "04o"), f"Invalid file mode: {name}")
        if name in BINARIES:
            require(record["source"] is None, "Executable cannot claim a source document blob")
        else:
            exact_keys(record["source"], ("path", "git_blob", "blob_url"), "source document")
            document = record["source"]
            relative = source_path(name)
            require(document["path"] == relative and isinstance(document["git_blob"], str)
                    and HEX_COMMIT.fullmatch(document["git_blob"])
                    and document["blob_url"] == f"{REPOSITORY}/blob/{commit}/{relative}",
                    f"Invalid source document linkage: {name}")
        records[name] = record
        total += record["bytes"]
    require(total <= MAX_TOTAL_BYTES, "Payload exceeds total size limit")
    require(list(records) == sorted(records), "Manifest inventory is not sorted")
    require(set(BINARIES) | set(SOURCE_FILES.values()) <= records.keys(), "Manifest omits required payload")
    require(any(is_notice(name) for name in records), "Manifest has no third-party notices")
    require(data == canonical_json(manifest), "Manifest is not canonical")
    return manifest, records


def tar_header(name, size):
    info = tarfile.TarInfo(name)
    info.size, info.mode = size, file_mode(name)
    return info.tobuf(format=tarfile.USTAR_FORMAT, encoding="ascii", errors="strict")


def archive_bytes(payload, manifest):
    output = io.BytesIO()
    for name, content in sorted({**payload, MANIFEST: canonical_json(manifest)}.items()):
        output.write(tar_header(name, len(content)))
        output.write(content)
        output.write(bytes(-len(content) % 512))
    output.write(bytes(1024))
    require(output.tell() <= MAX_TAR_BYTES, "Tar exceeds size limit")
    data = gzip_bytes(output.getvalue())
    require(len(data) <= MAX_ARCHIVE_BYTES, "Archive exceeds size limit")
    return data


def validate_guide_links(payload):
    try:
        text = payload[GUIDE].decode("utf-8")
    except (KeyError, UnicodeDecodeError) as error:
        raise BundleError("Missing or invalid UTF-8 Linux entry guide") from error
    for match in re.finditer(r"\[[^\[\]\r\n]+\]\(([^()\[\]\r\n]+)\)", text):
        target = match.group(1)
        if not target.startswith(("https://", "http://", "#")):
            require(target.split("#", 1)[0] in payload, f"Entry guide links unpackaged target: {target}")


def verify_bytes(data, expected_commit=None, expected_ci=None):
    require(18 <= len(data) <= MAX_ARCHIVE_BYTES and data[:10] == GZIP_HEADER,
            "Invalid or oversized gzip archive/header")
    try:
        inflater = zlib.decompressobj(-15)
        raw = inflater.decompress(data[10:-8], MAX_TAR_BYTES + 1)
        require(len(raw) <= MAX_TAR_BYTES and inflater.eof and not inflater.unused_data
                and not inflater.unconsumed_tail, "Incomplete, oversized or concatenated gzip stream")
        require(struct.unpack("<II", data[-8:]) == (zlib.crc32(raw) & 0xffffffff, len(raw)),
                "Gzip checksum or size mismatch")
    except (zlib.error, struct.error) as error:
        raise BundleError(f"Invalid gzip: {error}") from error
    payload, names, cursor, total = {}, set(), 0, 0
    while cursor + 512 <= len(raw) and raw[cursor:cursor + 512] != bytes(512):
        require(len(payload) < MAX_FILES, "Too many archive members")
        header = raw[cursor:cursor + 512]
        try:
            member = tarfile.TarInfo.frombuf(header, "ascii", "strict")
        except (tarfile.TarError, UnicodeError, ValueError) as error:
            raise BundleError("Invalid tar header") from error
        name = safe_name(member.name)
        require(name == MANIFEST or allowed_payload(name), f"Unexpected archive file: {name}")
        require(name.casefold() not in names, f"Duplicate or case-colliding archive path: {name}")
        names.add(name.casefold())
        require(member.type == tarfile.REGTYPE and 0 < member.size <= byte_limit(name),
                f"Non-regular or oversized tar member: {name}")
        require(header == tar_header(name, member.size), f"Unsafe or noncanonical tar metadata: {name}")
        start, end = cursor + 512, cursor + 512 + member.size
        cursor = end + (-member.size % 512)
        require(cursor <= len(raw) and raw[end:cursor] == bytes(cursor - end), "Truncated tar member or nonzero padding")
        total += member.size
        require(total <= MAX_TOTAL_BYTES + MAX_TEXT_BYTES, "Archive expansion exceeds limit")
        payload[name] = raw[start:end]
    require(raw[cursor:] == bytes(1024), "Tar must end with exactly two empty blocks; trailing data rejected")
    require(list(payload) == sorted(payload) and MANIFEST in payload, "Missing manifest or unsorted archive")
    manifest, records = validate_manifest(payload[MANIFEST], expected_commit, expected_ci)
    require(set(payload) == set(records) | {MANIFEST}, "Archive inventory differs from manifest")
    for name, record in records.items():
        content = payload[name]
        require(len(content) == record["bytes"], f"Size mismatch: {name}")
        require(hashlib.sha256(content).hexdigest() == record["sha256"], f"SHA256 mismatch: {name}")
        if name not in BINARIES:
            require(blob_hash(content) == record["source"]["git_blob"], f"Source blob mismatch: {name}")
    require(manifest["abi"] == {name: inspect_elf(payload[name], name) for name in BINARIES}, "Measured ELF ABI differs from manifest")
    validate_guide_links(payload)
    return manifest, payload


def verify_source(root, manifest, payload):
    """With a clean checkout available, anchor all document bytes to its commit."""
    root = checked_path(root)
    clean_commit(root, manifest["source"]["commit"])
    sources = tracked_sources(root)
    expected = {SOURCE_FILES.get(name, name) for name in sources if name != "Cargo.toml"}
    require(set(payload) == expected | set(BINARIES) | {MANIFEST}, "Source document inventory differs from archive")
    records = {item["path"]: item for item in manifest["files"]}
    for name, object_id in sources.items():
        content = source_bytes(root, name, object_id)
        if name == "Cargo.toml":
            cargo = tomllib.loads(content.decode("utf-8"))
            require(cargo.get("workspace", {}).get("package", {}).get("version") == manifest["version"],
                    "Source Cargo version differs from manifest")
        else:
            target = SOURCE_FILES.get(name, name)
            require(records[target]["source"]["git_blob"] == object_id and payload[target] == content,
                    f"Source document differs from committed blob: {name}")
    clean_commit(root, manifest["source"]["commit"])


def verify_extracted(payload, destination):
    require(os.name == "posix", "Checking Linux file modes requires a POSIX filesystem")
    destination = checked_path(destination)
    require(destination.is_dir(), "Extraction destination must be a directory")
    expected_directories = {parent.as_posix() for name in payload
                            for parent in Path(name).parents if parent != Path(".")}
    actual, actual_directories, pending = set(), set(), [destination]
    while pending:
        # scandir is incremental: an added directory with millions of entries
        # cannot make os.walk allocate an unbounded list before we reject it.
        with os.scandir(pending.pop()) as children:
            for child in children:
                path = checked_path(Path(child.path))
                info = path.lstat()
                name = path.relative_to(destination).as_posix()
                if stat.S_ISDIR(info.st_mode):
                    require(name in expected_directories and name not in actual_directories,
                            f"Unexpected extracted directory: {name}")
                    actual_directories.add(name)
                    pending.append(path)
                    continue
                require(name in payload and name not in actual and stat.S_ISREG(info.st_mode)
                        and info.st_nlink == 1 and stat.S_IMODE(info.st_mode) == file_mode(name),
                        f"Unexpected extracted file/mode/link: {name}")
                require(read_regular(path, byte_limit(name)) == payload[name], f"Extracted content changed: {name}")
                actual.add(name)
    require(actual == set(payload) and actual_directories == expected_directories,
            "Extracted inventory differs from archive")


def extract_payload(payload, destination):
    destination = checked_path(destination, leaf_missing=True)
    require(not destination.exists(), f"Extraction destination already exists: {destination}")
    destination.mkdir(mode=0o700)
    try:
        for name, content in sorted(payload.items()):
            safe_name(name)
            require(name == MANIFEST or allowed_payload(name), f"Unexpected extraction path: {name}")
            target = destination.joinpath(*name.split("/"))
            parent = destination
            for component in target.parent.relative_to(destination).parts:
                parent = checked_path(parent / component, leaf_missing=True)
                if not parent.exists():
                    parent.mkdir(mode=0o700)
            create_file(target, content, file_mode(name))
        verify_extracted(payload, destination)
    except BaseException:
        shutil.rmtree(destination)
        raise
    return destination


def receipt(path, data, manifest):
    return {"path": str(path), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
            "version": manifest["version"], "source_commit": manifest["source"]["commit"],
            "ci_run_url": manifest["ci"]["run_url"], "file_count": len(manifest["files"]) + 1,
            "abi": manifest["abi"]}


def build(root, binary_dir, output, source_commit, ci_run_url):
    require_build_host()
    root = checked_path(root)
    require(isinstance(ci_run_url, str) and RUN_URL.fullmatch(ci_run_url), "Invalid CI run URL")
    clean_commit(root, source_commit)
    sources = tracked_sources(root)
    cargo = tomllib.loads(source_bytes(root, "Cargo.toml", sources["Cargo.toml"]).decode("utf-8"))
    version = cargo.get("workspace", {}).get("package", {}).get("version")
    payload = {SOURCE_FILES.get(name, name): source_bytes(root, name, object_id)
               for name, object_id in sorted(sources.items()) if name != "Cargo.toml"}
    for name in BINARIES:
        path = checked_path(Path(binary_dir) / name)
        info = path.lstat()
        # Cargo normally hardlinks release outputs to its deps directory. Read
        # those stable bytes, but emit independent regular files on extraction.
        require(stat.S_ISREG(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o755,
                f"Executable input must be a regular 0755 file: {name}")
        payload[name] = read_regular(path, MAX_BINARY_BYTES)
    manifest = make_manifest(version, source_commit, ci_run_url, payload)
    validate_manifest(canonical_json(manifest))
    data = archive_bytes(payload, manifest)
    verified_manifest, verified_payload = verify_bytes(data, source_commit, ci_run_url)
    verify_source(root, verified_manifest, verified_payload)
    output = checked_path(output, leaf_missing=True)
    create_file(output, data)
    require(read_regular(output, MAX_ARCHIVE_BYTES) == data, "Written archive changed")
    return receipt(output, data, manifest)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    builder = commands.add_parser("build", help="Package existing matched normal default-feature release desktop and agent")
    builder.add_argument("--source-commit", required=True)
    builder.add_argument("--ci-run-url", required=True)
    builder.add_argument("--output", required=True, type=Path)
    builder.add_argument("--binary-dir", type=Path)
    verifier = commands.add_parser("verify", help="Verify and optionally extract into a fresh directory")
    verifier.add_argument("archive", type=Path)
    verifier.add_argument("--source-commit")
    verifier.add_argument("--ci-run-url")
    verifier.add_argument("--source-root", type=Path)
    verifier.add_argument("--extract-to", type=Path)
    verifier.add_argument("--verify-extracted", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "build":
            root = Path(__file__).absolute().parent.parent
            result = build(root, args.binary_dir or root / "target/release", args.output,
                           args.source_commit, args.ci_run_url)
        else:
            data = read_regular(args.archive, MAX_ARCHIVE_BYTES)
            manifest, payload = verify_bytes(data, args.source_commit, args.ci_run_url)
            if args.source_root is not None:
                verify_source(args.source_root, manifest, payload)
            result = receipt(args.archive.absolute(), data, manifest)
            if args.extract_to is not None:
                result["extracted_to"] = str(extract_payload(payload, args.extract_to))
            if args.verify_extracted is not None:
                verify_extracted(payload, args.verify_extracted)
                result["verified_extracted"] = str(args.verify_extracted.absolute())
        print(json.dumps(result, indent=2))
    except (BundleError, OSError, subprocess.SubprocessError, tomllib.TOMLDecodeError, UnicodeError) as error:
        print(f"Bundle error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
