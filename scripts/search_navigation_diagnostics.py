"""Strictly sanitized Project Search acceptance failure evidence.

The subprocess supervisor is shared unchanged with report triage. This adapter
has its own native test/source identity, fixed Check allowlist, and typed progress
schema. Neither direct-child reap nor progress can prove normal-agent cleanup;
only the complete success receipt can do that.
"""
import hashlib
import json
from pathlib import Path
import re

from copy_draft_acceptance import AcceptanceError, strict_json
from report_triage_diagnostics import sanitize_output as shared_compile_diagnostics
from report_triage_diagnostics import source_location

TEST_NAME = "search_navigation_process_tests::normal_agent_search_keyboard_navigation_is_local_and_source_bound"
SOURCE = "crates/app/src/search_navigation_process_tests.rs"
ARTIFACT_NAME = "SEARCH_NAVIGATION_FAILURE.json"
PROGRESS_PREFIX = "search_navigation_progress "
MAX_ARTIFACT_BYTES = 64 * 1024
MAX_DIAGNOSTICS = 32
PROGRESS_KEYS = {"schema", "completed_cases", "requests", "connections", "agents_reaped", "cleanup_uncertain"}
# Fixed reviewed messages, never learned from process output or source at runtime.
NATIVE_ERRORS = frozenset((
    'Back did not restore the exact departure selection',
    'Open operation ledger differed',
    'Undo target inactive',
    'accepted search source absent',
    'active local form absent',
    'agent connection failed; cleanup unverified and fixture retained',
    'app dispatched an operation other than List Read or Search',
    'both owned normal agents were not observed reaped',
    'captured old-query Enter retargeted an edited query',
    'case order changed',
    'completed Open retained a pending admission',
    'completed Open was not active',
    'connection did not dispatch exactly the initial root List',
    'different search lines shared one row identity',
    'dirty activation changed draft or baseline',
    'dirty activation or Back dispatched an operation',
    'dirty draft with full selection and native Undo was not prepared',
    'duplicate receipt field',
    'empty Search retained a result target',
    'expected open document absent',
    'explicit Open did not dispatch the exact ordinary Read',
    'explicit normal-agent selection missing',
    'explicit request missing',
    'explicit search did not dispatch the exact finite Search',
    'final exact Search Read and zero-write ledger differed',
    'finite fixture Search was truncated',
    'fixture directory creation failed',
    'fixture removal case list or total cleanup budget unverified',
    'fixture root absent',
    'fixture root absent during cleanup',
    'fixture root canonicalization failed',
    'fixture root creation failed',
    'fixture source creation failed',
    'fixture triggered optional system-font work',
    'focused search target was not revealed in its real viewport',
    'full editor selection absent',
    'generated fixture cleanup failed',
    'generated fixture root or form requires unsupported CJK font probing',
    'generated fixture temp path requires unsupported CJK font probing',
    'history document absent',
    'history observation exceeded native bound',
    'history state absent',
    'initial explicit search or setup ledger differed',
    'initial root List failed',
    'installed full selection differed',
    'keyboard focus did not retain exact search row identity',
    'late Search reply absent',
    'late old Search replaced the newer accepted batch',
    'native Redo lost dirty text or complete selection',
    'native Undo lost original full selection',
    'normal agent absent',
    'normal agent and source versions or platform differed',
    'normal agent disconnected during request',
    'normal agent was not ready with execution disabled',
    'normal-agent selection invalid',
    'old session Search or captured activation restored an active target',
    'one native Undo or Redo lost text history, baseline or save safety state',
    'ordinary Read reply differed',
    'ordinary Search reply differed',
    'owned agent absent',
    'owned agent cleanup unverified; generated fixture retained',
    'query input text differed',
    'read-only action changed a draft or saved baseline',
    'read-only action changed complete native history',
    'read-only action changed selection orientation or affinity',
    'ready Search reply could not enter the owner event queue',
    'real Search path line order or Unicode preview differed',
    'real Search result count differed',
    'receipt field count differed',
    'receipt object absent',
    'reconnect reused a search generation or request identity',
    'rendered search control did not receive focus',
    'replacement Search reused the old batch identity',
    'row movement dispatched an operation',
    'search activation did not complete the intentional line jump',
    'search navigation enabled execution, language or recovery work',
    'selection target absent',
    'selection target inactive',
    'shared acceptance budget exhausted',
    'source disk hash changed',
    'source reread failed',
    'stale race did not begin with a production-owned search key',
    'total cleanup budget exhausted',
    'unopened Read returned different fixture bytes',
    'unopened activation changed the original dirty draft',
    'unopened activation did not admit exactly one new document and Read',
    'unopened activation did not dispatch its one exact Read',
    'unsolicited or queued request was dispatched',
    'watchdog ownership absent; generated fixture retained',
    'watchdog stop unverified; generated fixture retained',
))


def progress_context(line):
    if not line.startswith(PROGRESS_PREFIX) or len(line.encode("utf-8")) > 1024:
        return None
    try:
        value = strict_json(line[len(PROGRESS_PREFIX):])
    except (AcceptanceError, ValueError, RecursionError):
        return None
    if type(value) is not dict or set(value) != PROGRESS_KEYS:
        return None
    for name, maximum in (("schema", 1), ("completed_cases", 14), ("requests", 12),
                          ("connections", 2), ("agents_reaped", 2)):
        if type(value[name]) is not int or not 0 <= value[name] <= maximum:
            return None
    if (value["schema"] != 1 or type(value["cleanup_uncertain"]) is not bool
            or value["agents_reaped"] > value["connections"]):
        return None
    return {"kind": "native_progress", **{name: value[name] for name in sorted(PROGRESS_KEYS)}}


def sanitize_output(data, root, stage):
    if stage == "compile":
        # This unchanged helper's compile branch accepts only compiler error
        # codes and verified relative Rust source locations, never report data.
        return shared_compile_diagnostics(data, root, stage)
    diagnostics, seen, suppressed, clipped = [], set(), 0, False
    known = {}
    if source_location(root, SOURCE, 1, 1) is not None:
        try:
            with (root / SOURCE).open("rb") as stream:
                source = stream.read(256 * 1024 + 1)
            if len(source) <= 256 * 1024:
                lines = source.decode("utf-8").splitlines()
                known = {message: [index for index, line in enumerate(lines, 1)
                                   if '"' + message + '"' in line]
                         for message in NATIVE_ERRORS}
        except (OSError, UnicodeError):
            pass
    for raw in data.splitlines():
        if len(raw) > 65536:
            suppressed += 1
            clipped = True
            continue
        try:
            line = raw.decode("utf-8")
        except UnicodeError:
            suppressed += 1
            continue
        line = line.removeprefix("test " + TEST_NAME + " ... ")
        item = progress_context(line) if stage == "runtime" else None
        native = re.fullmatch(r'Error: "([^"\r\n]+)"', line)
        if stage == "runtime" and native and known.get(native[1]):
            item = {"kind": "native_check", "message": native[1], "source": SOURCE}
            if len(known[native[1]]) == 1:
                item["line"] = known[native[1]][0]
        if stage == "runtime":
            panic = re.fullmatch(r"thread '[^'\r\n]{1,200}' panicked at (.{1,1024}):(\d{1,7}):(\d{1,6}):", line)
            if panic:
                location = source_location(root, panic[1], int(panic[2]), int(panic[3]))
                if location is not None:
                    item = {"kind": "panic_location", **location}
        if item is None:
            suppressed += 1
            continue
        encoded = json.dumps(item, sort_keys=True)
        if encoded in seen:
            continue
        seen.add(encoded)
        if len(diagnostics) == MAX_DIAGNOSTICS:
            clipped = True
        else:
            diagnostics.append(item)
    return diagnostics, suppressed, clipped


def failure_artifact(error):
    failure = error.failure
    diagnostics, suppressed, clipped = sanitize_output(failure.output, error.root, error.stage)
    return {
        "schema": 1, "acceptance": "search_navigation_diagnostic", "success": False,
        "stage": error.stage, "category": str(failure), "returncode": failure.returncode,
        "eof_observed": failure.eof, "capture_limit_bytes": error.limit,
        "captured_bytes": len(failure.output), "output_truncated": failure.truncated,
        "capture_complete": failure.eof and not failure.truncated,
        "timeout_seconds": error.timeout, "direct_child_cleanup": failure.cleanup,
        "native_cleanup": "unverified", "raw_output_omitted_bytes": len(failure.output),
        "raw_output_omitted_sha256": hashlib.sha256(failure.output).hexdigest(),
        "suppressed_lines": suppressed, "diagnostics_truncated": clipped,
        "diagnostics": diagnostics,
    }


def write_failure_artifact(error, directory):
    if directory is None:
        return "disabled"
    destination = Path(directory)
    if not destination.is_absolute():
        return "unavailable"
    try:
        current = destination
        while current != current.parent:
            if current.is_symlink():
                return "unavailable"
            current = current.parent
        destination.mkdir(mode=0o700, parents=False, exist_ok=True)
        if not destination.is_dir() or destination.resolve(strict=True) != destination:
            return "unavailable"
        data = (json.dumps(failure_artifact(error), ensure_ascii=True, sort_keys=True,
                           allow_nan=False, separators=(",", ":")) + "\n").encode()
        if len(data) > MAX_ARTIFACT_BYTES:
            return "unavailable"
        with (destination / ARTIFACT_NAME).open("xb") as stream:
            stream.write(data)
        return "written"
    except (OSError, ValueError):
        return "unavailable"
