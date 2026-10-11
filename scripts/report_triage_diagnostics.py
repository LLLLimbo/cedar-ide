"""Report-proof-only bounded capture and strictly allowlisted failure evidence.

Captured stdout and stderr are merged by the child pipe. Raw bytes are transient:
only fixed metadata, reviewed native Check messages, and verified relative Rust
locations can enter the bounded diagnostic artifact. No child output is echoed.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import time

from copy_draft_acceptance import AcceptanceError, require

REAP_TIMEOUT = 5
MAX_ARTIFACT_BYTES = 64 * 1024
MAX_DIAGNOSTICS = 32
ARTIFACT_NAME = "REPORT_TRIAGE_FAILURE.json"
STAGES = {"compile", "selection", "runtime"}
TEST_NAME = "report_triage_process_tests::normal_agent_report_keyboard_triage_is_local_and_revision_bound"
CATEGORIES = {
    "subprocess_launch_error", "subprocess_io_error", "subprocess_timeout",
    "subprocess_output_limit", "subprocess_nonzero", "output_pipe_missing",
    "child_cleanup_unverified", "unclassified_failure", "invalid_output_size",
    "output_validation_error", "output_validation_io_error",
    "invalid_output_encoding", "duplicate_json_key", "nonfinite_json", "invalid_json",
    "invalid_compiler_event", "invalid_compiler_target", "invalid_test_artifact",
    "unexpected_test_source", "unexpected_test_binary", "invalid_artifact_target",
    "artifact_source_mismatch", "exact_one_binary_required", "invalid_artifact_path",
    "compiled_test_missing", "exact_one_test_not_selected", "exact_one_test_not_executed",
    "exact_one_receipt_required", "invalid_receipt_prefix",
    "selected_test_execution_missing", "fixed_receipt_contract_mismatch",
    "wrong_test_artifact", "wrong_manifest", "wrong_source", "wrong_test_executable",
}
# Reviewed static errors from the generated-fixture process proof. Unknown
# assertion text, compiler prose, source snippets, and fixture values are omitted.
NATIVE_ERRORS = frozenset((
    'Open operation ledger differed',
    'activation did not select exact duplicate-name row and focus Back',
    'active local form absent',
    'agent connection failed; cleanup unverified and fixture retained',
    'app dispatched an operation other than List or Read',
    'both owned normal agents were not observed reaped',
    'captured old-filter movement retargeted a new visible row',
    'case order changed',
    'cleared name filter differed',
    'completed Open retained a pending admission',
    'completed Open was not active',
    'connection did not dispatch exactly the initial root List',
    'dirty draft with full selection and native Undo was not prepared',
    'duplicate names shared one row identity',
    'duplicate receipt field',
    'empty filter activated a row',
    'empty name filter retained a selectable detail target',
    'expected open document absent',
    'expected rendered label absent',
    'explicit Open did not dispatch the exact ordinary Read',
    'explicit normal-agent selection missing',
    'explicit report action did not dispatch its exact ordinary Read',
    'explicit request missing',
    'filter did not retain original XML indices in order',
    'final exact setup, refresh, reconnect or zero-navigation ledger differed',
    'fixture directory creation failed',
    'fixture removal, case list or total cleanup budget unverified',
    'fixture root absent',
    'fixture root absent during cleanup',
    'fixture root canonicalization failed',
    'fixture root creation failed',
    'fixture source creation failed',
    'fixture triggered optional system-font work',
    'focused report target was not revealed in its real viewport',
    'full editor selection absent',
    'generated fixture cleanup failed',
    'generated fixture root or form requires unsupported CJK font probing',
    'generated fixture temp path requires unsupported CJK font probing',
    'history document absent',
    'history observation exceeded native bound',
    'history state absent',
    'initial explicit report or setup ledger differed',
    'initial root List failed',
    'installed full selection differed',
    'keyboard focus did not retain the original XML row identity',
    'late old load replaced or selected the newer snapshot',
    'late ordinary Read absent',
    'native Redo lost dirty text or complete selection',
    'native Undo lost original full selection',
    'normal agent absent',
    'normal agent and source versions or platform differed',
    'normal agent disconnected during request',
    'normal agent was not ready with execution disabled',
    'normal-agent selection invalid',
    'old session Read or captured activation restored a target after reconnect',
    'one native Undo or Redo lost text history, baseline or save safety state',
    'ordinary Read reply differed',
    'ordinary report Read returned different fixture bytes',
    'owned agent absent',
    'owned agent cleanup unverified; generated fixture retained',
    'read-only action changed a draft or saved baseline',
    'read-only action changed complete native history',
    'read-only action changed selection orientation or affinity',
    'ready Read allowed captured activation to target a replacement snapshot',
    'ready ordinary Read could not enter the owner event queue',
    'receipt field count differed',
    'receipt object absent',
    'reconnect reused a report generation or load identity',
    'rendered report control did not receive focus',
    'report snapshot absent',
    'report triage enabled execution, language or recovery work',
    'selection target absent',
    'selection target inactive',
    'shared acceptance budget exhausted',
    'source disk hash changed',
    'source reread failed',
    'stale race did not begin with a production-owned report key',
    'total cleanup budget exhausted',
    'unsolicited or queued request was dispatched',
    'watchdog ownership absent; generated fixture retained',
    'watchdog stop unverified; generated fixture retained',
))

class CapturedFailure(AcceptanceError):
    """Transient bounded bytes; __str__ never includes child output."""
    def __init__(self, category, output=b"", *, returncode=None, eof=False,
                 cleanup="unverified", truncated=False):
        super().__init__(category if category in CATEGORIES else "unclassified_failure")
        self.output = output
        self.returncode = returncode
        self.eof = eof
        self.cleanup = cleanup
        self.truncated = truncated


class StageFailure(AcceptanceError):
    def __init__(self, stage, failure, root, timeout, limit):
        require(stage in STAGES, "unclassified_failure")
        super().__init__(stage + ":" + str(failure))
        self.stage, self.failure, self.root = stage, failure, root
        self.timeout, self.limit = timeout, limit


def bounded_process(command, root, environment, timeout, limit):
    """Same finite drain, EOF deadline and direct-child reap as shared driver.

    Report-only extension: retain the bounded merged stream on failure until it
    can be reduced to safe diagnostics. Never infer native-agent cleanup here.
    """
    started = time.monotonic()
    output = bytearray()
    eof, cleanup, category, truncated = False, "unverified", None, False
    try:
        process = subprocess.Popen(command, cwd=root, env=environment, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    except (OSError, ValueError):
        raise CapturedFailure("subprocess_launch_error", cleanup="not_started") from None
    try:
        require(process.stdout is not None, "output_pipe_missing")
        os.set_blocking(process.stdout.fileno(), False)
        while not eof or process.poll() is None:
            require(time.monotonic() - started < timeout, "subprocess_timeout")
            chunk = None
            if not eof:
                try:
                    chunk = os.read(process.stdout.fileno(), min(65536, limit - len(output) + 1))
                except BlockingIOError:
                    pass
                if chunk == b"":
                    eof = True
                elif chunk is not None:
                    remaining = limit - len(output)
                    output.extend(chunk[:remaining])
                    truncated = len(chunk) > remaining
                    require(not truncated, "subprocess_output_limit")
            if chunk is None:
                time.sleep(min(0.01, max(0, timeout - (time.monotonic() - started))))
        require(time.monotonic() - started < timeout, "subprocess_timeout")
        require(process.returncode == 0, "subprocess_nonzero")
    except AcceptanceError as error:
        category = str(error)
    except OSError:
        category = "subprocess_io_error"
    finally:
        try:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=REAP_TIMEOUT)
            cleanup = "reaped"
        except (OSError, subprocess.TimeoutExpired):
            category = "child_cleanup_unverified"
        finally:
            if process.stdout is not None:
                try:
                    process.stdout.close()
                except OSError:
                    category = category or "subprocess_io_error"
    if category is not None:
        raise CapturedFailure(category, bytes(output), returncode=process.returncode,
                              eof=eof, cleanup=cleanup, truncated=truncated) from None
    return bytes(output)


def run_stage(stage, root, command, environment, timeout, limit, validator, runner):
    """Bind a child and its output validation to one fixed stage, with no retry."""
    try:
        output = runner(command, root, environment, timeout, limit)
    except CapturedFailure as failure:
        raise StageFailure(stage, failure, root, timeout, limit) from None
    except AcceptanceError as error:
        failure = CapturedFailure(str(error))
        raise StageFailure(stage, failure, root, timeout, limit) from None
    try:
        return validator(output)
    except AcceptanceError as error:
        failure = CapturedFailure(str(error), output, returncode=0, eof=True, cleanup="reaped")
        raise StageFailure(stage, failure, root, timeout, limit) from None
    except (OSError, ValueError, RecursionError) as error:
        category = "output_validation_io_error" if isinstance(error, OSError) else "output_validation_error"
        failure = CapturedFailure(category, output, returncode=0, eof=True, cleanup="reaped")
        raise StageFailure(stage, failure, root, timeout, limit) from None


def source_location(root, name, line, column):
    """Only an existing nonsymlink Rust source inside this exact checkout."""
    if (type(name) is not str or len(name) > 1024 or type(line) is not int
            or type(column) is not int or not 1 <= line <= 1000000 or not 1 <= column <= 100000):
        return None
    try:
        root = root.resolve(strict=True)
        candidate = Path(name)
        if candidate.is_absolute():
            relative = candidate.relative_to(root)
        else:
            relative = candidate
        normalized = relative.as_posix()
        if (len(normalized) > 160 or not re.fullmatch(
                r"crates/[a-z0-9_-]+/(?:src|tests)/(?:[A-Za-z0-9_-]+/)*[A-Za-z0-9_-]+\.rs", normalized)):
            return None
        current = root
        for part in relative.parts:
            current = current / part
            if current.is_symlink():
                return None
        if not current.is_file() or current.resolve(strict=True) != root / relative:
            return None
        return {"source": normalized, "line": line, "column": column}
    except (OSError, ValueError):
        return None


def native_errors(root):
    """Intersect reviewed literals with bounded current source; never learn prose."""
    name = "crates/app/src/report_triage_process_tests.rs"
    if source_location(root, name, 1, 1) is None:
        return {}
    try:
        with (root / name).open("rb") as stream:
            data = stream.read(256 * 1024 + 1)
        if len(data) > 256 * 1024:
            return {}
        lines = data.decode("utf-8").splitlines()
    except (OSError, UnicodeError):
        return {}
    return {message: [index for index, line in enumerate(lines, 1)
                      if '"' + message + '"' in line]
            for message in NATIVE_ERRORS
            if any('"' + message + '"' in line for line in lines)}


def sanitize_output(data, root, stage):
    """Reconstruct allowlisted facts; no original output line is copied."""
    diagnostics, seen, suppressed, clipped = [], set(), 0, False
    known = native_errors(root) if stage == "runtime" else {}

    def add(item):
        nonlocal clipped
        encoded = json.dumps(item, ensure_ascii=True, sort_keys=True)
        if encoded in seen:
            return
        if len(diagnostics) == MAX_DIAGNOSTICS:
            clipped = True
            return
        seen.add(encoded)
        diagnostics.append(item)

    for raw_line in data.splitlines():
        if len(raw_line) > 65536:
            clipped = True
            suppressed += 1
            continue
        try:
            line = raw_line.decode("utf-8")
        except UnicodeError:
            suppressed += 1
            continue
        accepted = False
        if stage == "runtime":
            line = line.removeprefix("test " + TEST_NAME + " ... ")
        native = re.fullmatch(r'Error: "([^"\r\n]+)"', line)
        if native and native[1] in known:
            item = {"kind": "native_check", "message": native[1],
                    "source": "crates/app/src/report_triage_process_tests.rs"}
            if len(known[native[1]]) == 1:
                item["line"] = known[native[1]][0]
            add(item)
            accepted = True
        if stage == "runtime":
            panic = re.fullmatch(r"thread '[^'\r\n]{1,200}' panicked at (.{1,1024}):(\d{1,7}):(\d{1,6}):", line)
            if panic:
                location = source_location(root, panic[1], int(panic[2]), int(panic[3]))
                if location:
                    add({"kind": "panic_location", **location})
                    accepted = True
        if stage == "compile" and line.startswith("{"):
            try:
                value = json.loads(line)
            except (ValueError, RecursionError):
                value = None
            if type(value) is dict and value.get("reason") == "compiler-message":
                message = value.get("message")
                if type(message) is dict and message.get("level") == "error":
                    code, spans = message.get("code"), message.get("spans")
                    code = code.get("code") if type(code) is dict else None
                    if type(code) is str and re.fullmatch(r"E[0-9]{4}", code):
                        locations = []
                        if type(spans) is list:
                            clipped |= len(spans) > 32
                            for span in spans[:32]:
                                if type(span) is dict:
                                    location = source_location(root, span.get("file_name"),
                                                               span.get("line_start"), span.get("column_start"))
                                    if location and location not in locations:
                                        if len(locations) < 4:
                                            locations.append(location)
                                        else:
                                            clipped = True
                        add({"kind": "compiler_error", "code": code, "locations": locations})
                        accepted = True
        if not accepted:
            suppressed += 1
    return diagnostics, suppressed, clipped


def failure_artifact(error):
    import hashlib
    failure = error.failure
    # Every raw byte is omitted; the hash binds this sanitized evidence to the
    # exact bounded capture without copying a source snippet or private value.
    diagnostics, suppressed, clipped = sanitize_output(failure.output, error.root, error.stage)
    safe = json.dumps(diagnostics, ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode()
    return {
        "schema": 1, "acceptance": "report_triage_diagnostic", "success": False,
        "stage": error.stage, "category": str(failure), "returncode": failure.returncode,
        "eof_observed": failure.eof, "capture_limit_bytes": error.limit,
        "captured_bytes": len(failure.output), "output_truncated": failure.truncated,
        "capture_complete": failure.eof and not failure.truncated,
        "timeout_seconds": error.timeout, "direct_child_cleanup": failure.cleanup,
        "native_cleanup": "unverified", "raw_output_omitted_bytes": len(failure.output),
        "raw_output_omitted_sha256": hashlib.sha256(failure.output).hexdigest(),
        "sanitized_diagnostic_bytes": len(safe), "suppressed_lines": suppressed,
        "diagnostics_truncated": clipped, "diagnostics": diagnostics,
    }


def write_failure_artifact(error, directory):
    """Explicit CI-only destination, exclusive create, no overwrite or raw dump."""
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
