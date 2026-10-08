#!/usr/bin/env python3
"""Measure native environment-name equivalence using isolated synthetic children.

Only SystemRoot and one fixed synthetic variable enter each child. Output has
case IDs, Win32 result/error integers and booleans, never environment strings.
Agreement is required independently of the real-Git redirect/trace acceptance.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import subprocess
import sys


VALUE = "cedar-environment-contract-v1"
CASES = (
    ("GIT_INDEX_FILE", "GIT_INDEX_FILE"),
    ("gIt_InDeX_fIlE", "GIT_INDEX_FILE"),
    ("GıT_INDEX_FILE", "GIT_INDEX_FILE"),
    ("GıT_TRACE", "GIT_TRACE"),
    ("gıt_dır", "GIT_DIR"),
    ("cedar_σ", "cedar_Σ"),
    ("雪_KEY", "GIT_INDEX_FILE"),
)
KEYS = {"case_id", "ordinal_result", "canonical_lookup_found",
        "canonical_lookup_matches", "canonical_lookup_error",
        "exact_lookup_verified", "raw_spelling_verified"}
CURRENT_CASE = -1


def require(condition, code):
    if not condition:
        raise ValueError(code)


def native():
    from ctypes import wintypes as w
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetEnvironmentVariableW.argtypes = [w.LPCWSTR, w.LPWSTR, w.DWORD]
    kernel.GetEnvironmentVariableW.restype = w.DWORD
    kernel.GetEnvironmentStringsW.argtypes = []
    kernel.GetEnvironmentStringsW.restype = ctypes.POINTER(ctypes.c_wchar)
    kernel.FreeEnvironmentStringsW.argtypes = [ctypes.POINTER(ctypes.c_wchar)]
    kernel.FreeEnvironmentStringsW.restype = w.BOOL
    kernel.CompareStringOrdinal.argtypes = [w.LPCWSTR, ctypes.c_int, w.LPCWSTR,
                                            ctypes.c_int, w.BOOL]
    kernel.CompareStringOrdinal.restype = ctypes.c_int
    return kernel


def lookup(kernel, name):
    buffer = ctypes.create_unicode_buffer(128)
    ctypes.set_last_error(0)
    length = kernel.GetEnvironmentVariableW(name, buffer, len(buffer))
    error = ctypes.get_last_error()
    require(length < len(buffer), "lookup_bound")
    found = length != 0
    require(found or error == 203, "lookup_failure")  # ERROR_ENVVAR_NOT_FOUND
    return found, found and buffer.value == VALUE, 0 if found else error


def child(case_id):
    require(0 <= case_id < len(CASES), "case_id")
    alias, canonical = CASES[case_id]
    kernel = native()
    block = kernel.GetEnvironmentStringsW()
    require(bool(block), "environment_block_unavailable")
    raw_matches = 0
    try:
        entry = []
        terminated = False
        for index in range(32768):
            char = block[index]
            if char != "\0":
                entry.append(char)
                continue
            if not entry:
                terminated = True
                break
            raw_matches += "".join(entry) == alias + "=" + VALUE
            entry.clear()
        require(terminated, "environment_block_bound")
    finally:
        require(kernel.FreeEnvironmentStringsW(block), "environment_block_release")
    exact_found, exact_matches, _ = lookup(kernel, alias)
    found, matches, error = lookup(kernel, canonical)
    alias_units = len(alias.encode("utf-16-le", errors="surrogatepass")) // 2
    canonical_units = len(canonical.encode("utf-16-le", errors="surrogatepass")) // 2
    ordinal = kernel.CompareStringOrdinal(alias, alias_units, canonical, canonical_units, True)
    require(ordinal in (1, 2, 3), "ordinal_failure")
    print(json.dumps({
        "case_id": case_id,
        "ordinal_result": ordinal,
        "canonical_lookup_found": found,
        "canonical_lookup_matches": matches,
        "canonical_lookup_error": error,
        "exact_lookup_verified": exact_found and exact_matches,
        "raw_spelling_verified": raw_matches == 1,
    }, sort_keys=True))


def main():
    global CURRENT_CASE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--child", type=int)
    args = parser.parse_args()
    require(os.name == "nt", "native_windows_required")
    if args.child is not None:
        CURRENT_CASE = args.child
        child(args.child)
        return 0
    before = dict(os.environ)
    system_root = os.environ.get("SystemRoot")
    require(isinstance(system_root, str) and 0 < len(system_root) <= 4096,
            "system_root_required")
    records = []
    agreement = True
    transport_verified = True
    controls_verified = True
    for case_id, (alias, _) in enumerate(CASES):
        CURRENT_CASE = case_id
        result = subprocess.run(
            [sys.executable, "-I", "-S", "-B", str(Path(__file__).resolve()), "--child", str(case_id)],
            env={"SystemRoot": system_root, alias: VALUE}, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=10, check=False,
        )
        require(result.returncode == 0 and 0 < len(result.stdout) <= 4096, "child_failed")
        record = json.loads(result.stdout)
        require(isinstance(record, dict) and set(record) == KEYS, "child_schema")
        require(type(record["case_id"]) is int and record["case_id"] == case_id,
                "child_case")
        require(type(record["ordinal_result"]) is int and record["ordinal_result"] in (1, 2, 3),
                "child_ordinal")
        require(type(record["canonical_lookup_error"]) is int
                and record["canonical_lookup_error"] in (0, 203), "child_error")
        for key in KEYS - {"case_id", "ordinal_result", "canonical_lookup_error"}:
            require(type(record[key]) is bool, "child_boolean")
        transport_verified = (transport_verified and record["raw_spelling_verified"]
                              and record["exact_lookup_verified"]
                              and record["canonical_lookup_found"] == record["canonical_lookup_matches"])
        equal = record["ordinal_result"] == 2
        agrees = record["canonical_lookup_found"] == equal
        if case_id in (0, 1, 5):
            controls_verified = controls_verified and equal and record["canonical_lookup_matches"]
        if case_id == 6:
            controls_verified = controls_verified and not equal and not record["canonical_lookup_found"]
        agreement = agreement and agrees
        records.append(record)
    parent_unchanged = before == dict(os.environ)
    passed = agreement and transport_verified and controls_verified and parent_unchanged
    print(json.dumps({"schema_version": 1, "suite": "windows_environment_contract",
                      "result": "PASS" if passed else "FAIL",
                      "comparison_matches_lookup": agreement,
                      "child_spelling_and_value_verified": transport_verified,
                      "controls_verified": controls_verified,
                      "parent_environment_unchanged": parent_unchanged,
                      "cases": records}, sort_keys=True))
    return 0 if passed else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:
        # Native setup failures must not expose a raw environment or traceback.
        print(json.dumps({"suite": "windows_environment_contract", "result": "FAIL",
                          "case_id": CURRENT_CASE, "code": "bounded_probe_failed"}, sort_keys=True))
        sys.exit(1)
