# Verification report · phase 3 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar **0.3.0**, frontend/agent protocol **3**, is a tested development checkpoint,
not a complete IntelliJ IDEA replacement. This phase adds private frontend-local
draft recovery and bounded asynchronous command tasks. Incompatible protocol
versions are rejected. [Phase 2](TEST_REPORT_PHASE2.md) is preserved as an
unchanged historical report; [phase 1](TEST_REPORT_PHASE1.md) and Git tag
`phase1-0.1.0` retain earlier evidence.

## Current aggregate results

Environment: Linux x86_64, kernel 6.18.44, glibc 2.41, rustc 1.99.0.

| Check | Phase-3 result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| Strict workspace Clippy, all targets and features, locked dependencies | PASS |
| Workspace tests, all targets and features | **244 ordinary tests passed**, zero failures, 4 opt-in tests ignored |
| Explicit client → separate-agent process test | **1 additional PASS**; this is one of the 4 tests ignored in the aggregate |
| Python black-box filesystem/trust/conflict/EOF/malformed-frame smoke | PASS |
| Python agent → LSP lifecycle/completion/resolve/navigation smoke | PASS |
| Python asynchronous agent task/live-output/edit/cancel smoke | PASS |
| Full Linux release workspace build | PASS |
| Windows MSVC full-workspace/all-target/all-feature `cargo check` | PASS; **no Windows execution or final link** |

Thus the Rust count is **244 + 1 = 245 passed** across the aggregate and explicit
agent invocation, plus three Python smoke scripts. Do not add the 4 ignored
cases to the passed count. Real JDT LS, installed-system-font and real debugpy
checks remain opt-in and were not rerun as part of this phase's aggregate;
their phase-2 runs are historical evidence below.

The complete `scripts/verify.sh` passed. Raw logs:
[verification-phase3-log.txt](../PUBLICATION.md#verification-evidence),
[release-phase3-build.txt](../PUBLICATION.md#verification-evidence),
[windows-phase3-check.txt](../PUBLICATION.md#verification-evidence).
The supplied CI workflow has not run on an external service.

## Native Linux desktop checks in this phase

A real OS window was operated with keyboard and pointer input on the Linux
cloud desktop. The forced-crash and asynchronous command checks below used the
debug binary; a separate release-binary window/recovery pass is recorded after
them. These checks are distinct from headless UI/state tests. Inputs were
synthetic fixtures; no user's server or project was accessed.

### Exact draft recovery after forced process termination

1. Edited a Chinese draft in the native editor, leaving it unsaved
2. Waited for **Draft backed up locally**, the acknowledgement for that exact
   draft version rather than merely a queued write
3. Verified the owned frontend PID and terminated that process with SIGKILL
4. Restarted Cedar and reviewed the available recovery copy; then explicitly
   connected with execution trust **off** and restored it
5. Verified the buffer matched the original unsaved draft exactly
6. Before restoration, externally changed the synthetic workspace file; after
   restoration, Ctrl+S produced a conflict instead of overwriting the changed
   file. Both the external disk content and recovered draft remained intact
7. Explicitly discarded and quit, verified the owned recovery record was
   removed, and restored the original test fixture

Screenshot: [native-recovery-phase3.png](../PUBLICATION.md#verification-evidence).

This proves process-crash recovery of an acknowledged snapshot on the tested
Linux filesystem. It does not prove power-loss durability, recovery of input
after the last acknowledgement, or Windows/macOS behavior. Recovery is local
plaintext backup, not project autosave or an encrypted secrets store.

### Live command output with concurrent editing and cancellation

- In an explicitly trusted workspace, launched executable `sh` with literal
  argv `["run-demo.sh"]`; the synthetic script printed output and slept for
  180 seconds
- Observed live command output and the **Running** state while the command was
  still active
- Opened another note, typed and saved it while the command remained Running;
  independently verified the saved disk contents
- Attempted reconnect while running and observed the **Cancel-and-wait** guard
- Dismissed that guard, then used **Commands → Cancel**; a fresh native screenshot
  confirmed the terminal **Cancelled** state
- Reconnected after terminal status; the saved buffer remained available and
  the command did not restart automatically

Screenshots: [native-tasks-running-phase3.png](../PUBLICATION.md#verification-evidence)
and [native-tasks-cancelled-phase3.png](../PUBLICATION.md#verification-evidence).
The guard's appearance was manually tested; its own Cancel-and-wait button was
**not** the cancellation path exercised in this session. Ordinary task-manager,
agent and UI-state tests separately cover cancellation behavior and transition
protection. A cancellation request alone is never reported here as terminal
cancellation.

### Separate release-binary window and recovery check

The optimized `target/release/cedar` was also operated in a native window for
112.597 seconds. It opened a CJK `note.txt` with trust off, no JVM and default
recovery on. After typing unsaved text, the exact backup acknowledgement was
observed; dirty-close required explicit discard and the owned recovery record's
removal was verified. The fixture disk content was unchanged and the process
exited with code 0. This validates release window/recovery interaction, not a
repeat of the debug-binary forced-kill or running-command sessions.

Direct frontend-PID sampling collected 1,093 readings: sampled RSS maximum
117,908 KiB (**115.14 MiB**), observed HWM 120,800 KiB (**117.97 MiB**), and maximum
23 threads. Raw evidence:
[gui-phase3-cjk-recovery-resource-sample.json](../PUBLICATION.md#verification-evidence).
Separate JVM/adapter processes and GPU-service memory are excluded; child
rusage is not a concurrent process-tree total. Different interactions prevent
an overhead or improvement comparison against phase 2. There is no IDEA
comparison, fixed memory budget, long-session leak test or production benchmark.

## New automated recovery coverage

The recovery crate contributes **32 passing tests** (one unit test and 31
storage/integration tests). They exercise:

- Exact Unicode, CRLF/BOM and saved/base revision round trips; new files retain
  an absent base revision; all SSH identity fields distinguish records
- No workspace modification or connection while storing remote drafts;
  startup metadata listing without eagerly reading every payload
- Real separate-process lock contention and actual process termination after
  durable acknowledgement, followed by lock reacquisition and exact recovery
- Atomic replacement, monotonic mutation ordering, deletion tombstones,
  failed-write retry ordering and bounded per-process sequence memory
- Corrupt, truncated, oversized and unsupported records; metadata/payload
  checksums, parse-valid revision/timestamp damage, invalid UTF-8 and strict
  portable path validation
- Quota/headroom/entry exhaustion without eviction or false acknowledgement;
  retained unknown files and interrupted temporaries
- Unix private modes, symlinks, hard links, special files, root/lock replacement,
  and retry of parent-directory synchronization after an injected sync failure

Frontend regression tests additionally cover coalescing and exact-version
acknowledgements, save-while-typing, explicit restore with original revisions,
trust-off connection, older unreviewed copy protection, tab ownership,
workspace/session changes, full-store behavior, discard/removal waits and new
input cancelling a pending close. The UI renders recovery errors and review
controls in headless layout tests as well as the native session above.

See [RECOVERY.md](RECOVERY.md) and
[frontend recovery/commands](../crates/app/RECOVERY_AND_COMMANDS.md) for bounds
and durability limits. File flush and Unix directory barriers do not establish
physical power-loss safety on every filesystem or storage device. Windows
inherits ACLs and lacks the same containing-directory flush guarantee; cross-
compilation is not runtime validation.

## New automated asynchronous task coverage

The task crate contributes **24 passing tests** (9 unit and 15 real-process
integration tests). Coverage includes:

- Prompt acceptance/polling, partial output before exit, literal argv/cwd,
  null stdin, success/nonzero exit and retained spawn failure without retry
- Timeout versus explicit cancellation, repeated cancellation racing natural
  exit, and old task cancellation never targeting a newer task
- Independent stdout/stderr floods, raw-byte caps, split Unicode, bounded
  error text, retained-history eviction and manager-scoped nonreused IDs
- Concurrent start rejection, manager-drop cleanup, normal leader exit with
  descendants holding pipes, nonblocking bounded final draining
- `WNOWAIT` observation before group cleanup/reaping, lost wait ownership,
  unwind cleanup, real read errors and injected interrupted/would-block reads

Workspace tests enforce the trust gate and lazy supervisor creation, and prove
filesystem operations remain available until cancellation finishes. The real
frontend-worker test opens/saves files while a real process streams output and
then cancels it. UI-state tests reject stale connection/task results and cover
unknown outcomes, transition guards and bounded full-snapshot presentation.

The separate Python `task_bridge_smoke.py` drives the **actual stdio agent**:
start, live output, concurrent file reads/writes, busy rejection, cancel and
repeated cancel, success, spawn failure, unknown IDs and EOF. It is evidence for
the shared local/remote protocol path, **not authenticated SSH/network testing**.

The supervisor cleans ordinary owned process groups on tested Linux. Deliberate
session/group escape, privilege changes and uninterruptible kernel operations
are outside that guarantee. Windows local command execution remains disabled;
macOS is compile-path implementation without runtime evidence. Details:
[RUN_TASKS.md](RUN_TASKS.md).

## Retained regressions

The full suite continues to check file revision conflicts, traversal/symlink
rejection, bounded search, dirty-close protection, save acknowledgements versus
newer drafts, and stale workspace generations. LSP tests retain bounded queues,
timeouts, UTF-16/CRLF edit validation, atomic completion/import undo, safe URI
navigation, session/document/edit stamps and rejection of arbitrary server
commands. Deterministic DAP transport tests also remain in the aggregate.

## Historical phase-2 evidence, not rerun claims

These results remain useful but must not be presented as new phase-3 real-server
or performance measurements. Full detail and limits are preserved unchanged in
[TEST_REPORT_PHASE2.md](TEST_REPORT_PHASE2.md) and its referenced raw evidence.

### Java and Chinese rendering

Phase 2 exercised official JDT LS 1.61.0 / OpenJDK 21.0.12.1 through the real
native editor: automatic diagnostics; resolved GregorianCalendar completion
with deferred import as one unsaved transaction; atomic undo/redo; correction
of a type error; F12 definition and String hover; explicit dirty-session discard
and server shutdown. Disk remained unchanged by unsaved completion edits. The
corrected example retained two unused-local warnings, not zero diagnostics.

Evidence: [native-java-phase2.png](../PUBLICATION.md#verification-evidence),
`crates/app/tests/evidence/jdtls-1.61.0-editor.json` and
[LANGUAGE_SERVICES.md](LANGUAGE_SERVICES.md).
The system CJK font test and [native-cjk-phase2.png](../PUBLICATION.md#verification-evidence)
validated existing Noto Sans Mono CJK SC glyphs without downloading or bundling
fonts. Phase 3's native recovery check also displayed a Chinese draft; it was
not a rerun of the explicit font-only test.

### Kotlin and debugging foundation

Phase 2's deprecated MIT-licensed fwcd Kotlin server 1.3.13 (compiler 2.1.0)
passed actual type diagnostics, hover, completion, definition and correction
through the Rust client. Shutdown required forced reaping and logged an upstream
disposal error. This is not a current Kotlin or production-project compatibility
pass. The official 263.6379.0 archive was checksum-verified but required an
explicit EULA while reporting its referenced EULA.txt missing. No acceptance
or semantic session was attempted. See [KOTLIN_VALIDATION.md](KOTLIN_VALIDATION.md).

The separate debugger crate's historical debugpy 1.8.22 session verified a real
Python breakpoint, thread/stack/scope, local answer=41, resume/output answer=42
and termination, with graceful/crash/drop checks. It is not an integrated GUI
or remote debugger. General descendant cleanup and debugpy's additional
unauthenticated loopback endpoint remain integration blockers. Java debugging
is absent. See [DEBUGGING.md](DEBUGGING.md).

### Historical performance evidence remains separate

The current release smoke sample is reported above. No controlled phase-3
resource benchmark or IDEA comparison was performed. The phase-2
Chinese-capable **frontend-only** release sample measured 118.52 MiB observed
RSS/HWM on a tiny fixture. The earlier phase-1 78.44 MiB sample predates that
feature set and is not its footprint. Neither includes JVM or GPU-service
memory, and neither is a hard budget or leak test.

Phase-2 JDT LS/JVM-only measurements and heap/GC experiments are reported
separately in [PERFORMANCE.md](PERFORMANCE.md) and
[jvm-profile-experiments/REPORT.md](jvm-profile-experiments/REPORT.md). They cannot
be added to a different frontend session as a measured simultaneous total.
Moving JVM work to a remote host relocates its cost; it does not eliminate it
or establish a quantified advantage over IntelliJ IDEA.

## Still unverified or missing

- Authenticated real SSH sessions, network-failure interoperability and remote
  task outcome reconciliation across lost/restarted agent sessions
- Windows final link/runtime and Job Object command support; macOS runtime;
  platform recovery/privacy/locking behavior; physical power-loss durability
- Complete IME/accessibility/high-DPI acceptance and sustained production use
- Safe formatting previews, references and document outline; multi-file rename
  with reliable snapshots/resource operations; general workspace edits, code
  actions, snippets, full semantic refactoring and project indexing
- Production Maven/Gradle/Kotlin imports, JDK management and official Kotlin
  license/setup resolution
- Integrated debugger UI/remote lifecycle and listener security, PTY, test tree,
  full Git workflows, stable plugins, signed installation and deployment
- Controlled equivalent-feature performance comparison and long-session
  resource/leak testing

The next proposed phase prioritizes validated single-document formatting
previews, references and document outline; these features remain unimplemented.
Multi-file rename is deferred until unversioned cross-file responses have a
trustworthy snapshot design and potentially omitted file resource operations
can be handled safely. See [FEATURE_MATRIX.md](FEATURE_MATRIX.md)
for the wider gap and proposed acceptance order.
