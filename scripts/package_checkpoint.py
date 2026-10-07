#!/usr/bin/env python3
"""Archive a clean tagged source checkpoint and its already-built Linux binaries.

This does not build, publish, or claim reproducible binaries. Run verify.sh and
the release build first. The archive records the exact source and binary hashes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if git(root, "status", "--porcelain").strip():
        parser.error("Commit all intended source and evidence before packaging")
    commit = git(root, "rev-parse", "HEAD").decode().strip()
    tag = git(root, "describe", "--tags", "--exact-match", "HEAD").decode().strip()
    tracked = git(root, "ls-files", "-z").decode().split("\0")[:-1]
    binaries = [root / "target/release" / name for name in ("cedar", "cedar-agent")]
    for path in binaries:
        if not path.is_file() or path.read_bytes()[:4] != b"\x7fELF":
            parser.error(f"Expected a previously verified Linux ELF binary: {path}")
    snapshot = {
        "source_commit": commit,
        "source_tag": tag,
        "verification_report": "docs/TEST_REPORT.md",
        "binaries": {
            path.name: {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                        "bytes": path.stat().st_size}
            for path in binaries
        },
        "binary_platform": "Linux x86_64, cloud runtime tested; unsigned",
        "windows": "Compilation checked only; no executable or runtime validation",
        "remote_ssh": "Protocol chain tested; authenticated SSH not runtime tested",
        "reproducible_build_claim": False,
    }
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for relative in tracked:
            path = root / relative
            if path.is_symlink() or not path.is_file():
                parser.error(f"Tracked source is not a regular file: {relative}")
            archive.write(path, "cedar-ide/" + relative)
        for path in binaries:
            archive.write(path, "cedar-ide/bin/linux-x86_64/" + path.name)
        archive.writestr("cedar-ide/BUILD_SNAPSHOT.json", json.dumps(snapshot, indent=2) + "\n")
        archive.writestr("cedar-ide/bin/README.txt", "Unsigned Linux x86_64 checkpoint binaries.\n"
                          "Prefer building from source for your target system.\n"
                          "The desktop frontend requires a graphical session and OpenGL.\n"
                          "No Windows executable, JDK, language server or debug adapter is bundled.\n")
    with zipfile.ZipFile(output) as archive:
        bad = archive.testzip()
        if bad is not None:
            raise RuntimeError(f"Archive checksum failed: {bad}")
    print(json.dumps({"path": str(output), "bytes": output.stat().st_size,
                      "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                      "source_commit": commit, "source_tag": tag}, indent=2))


if __name__ == "__main__":
    main()
