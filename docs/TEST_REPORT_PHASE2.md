# Verification report · phase 2 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar 0.2.0 is a tested development checkpoint, not a complete IntelliJ IDEA
replacement. Phase 1 is preserved by Git tag `phase1-0.1.0` and
`TEST_REPORT_PHASE1.md`. The current frontend/agent protocol version is 2;
version mismatches are rejected.

## Aggregate results

Environment: Linux x86_64, kernel 6.18.44, glibc 2.41, rustc 1.99.0.

- Formatting and strict all-target/all-feature Clippy: PASS
- All-target/all-feature workspace suite: **152 ordinary tests passed**, zero failures; environment-dependent real-server/font checks remain explicitly ignored in this aggregate
- Explicit client→separate-agent process test: **1 additional PASS**
- Black-box agent filesystem/trust/conflict/EOF/malformed-frame smoke: PASS
- Agent→LSP process-chain smoke including completion resolution and safe URI navigation: PASS
- Complete release workspace build: PASS
- Complete Windows MSVC target, all-target/all-feature compilation check: PASS. Windows execution and final linking were not performed

Raw logs: `verification-phase2-log.txt`, `windows-phase2-check.txt`.
The provided CI workflow has not run on an external service.

## Native Linux desktop checks

A real OS window was operated with actual keyboard and pointer input. This is
separate from the automated/headless app-state tests.

### Phase-1 behavior revalidated by code regressions

Local connection, directory navigation, multiple Java/Kotlin tabs, typing,
Ctrl+S disk writes, dirty-tab/dirty-window confirmation, external-change save
conflict and draft retention were exercised in the original native session.
Those behaviors remain covered by the expanded regression suite.

### Actual JDT LS session through the desktop

The `examples/java-language-demo` Eclipse fixture deliberately begins with a
missing GregorianCalendar import and a String-to-int assignment error.

- Explicitly started official JDT LS 1.61.0 in the UI; the two real diagnostics appeared automatically without pressing Sync
- Ctrl+Space inside `GregorianCalendar` returned actual JDT completion candidates
- Enter resolved the selected candidate and applied its primary edit plus deferred import as one unsaved transaction
- Disk bytes were checked and remained unchanged
- One Ctrl+Z removed the entire operation; Ctrl+Shift+Z restored both edits
- Editing `"oops"` to `42` removed the type error automatically; two expected unused-local warnings remained, not falsely reported as a completely clean file
- F12 on `greeting` selected the local declaration, using agent-confined URI resolution
- Ctrl+K displayed its real String type hover
- Closing the dirty session required an explicit discard decision, then shut down the server; the original fixture remained unchanged

Screenshot: `native-java-phase2.png`. A separate automated real-JDT frontend
transaction test checks the same protocol/atomic-edit/undo path; evidence:
`crates/app/tests/evidence/jdtls-1.61.0-editor.json`.

### Chinese rendering

The original default fonts displayed Chinese source as missing-glyph squares.
A bounded lazy system-font fallback was implemented. The real egui glyph test
and native screenshot now show the full Chinese comment and `你好，世界` in
`UnicodeDemo.java`, including when workspace execution trust is disabled.

The Linux font is the existing Noto Sans Mono CJK SC collection face. It was not
downloaded or bundled. File reads are off the UI thread, capped at 32 MiB, and
validated. Screenshot: `native-cjk-phase2.png`. Windows/macOS lookup paths are
implemented but remain unverified on those systems.

## Real server/adapter validation beyond GUI checks

### Java

Official JDT LS 1.61.0 milestone, OpenJDK21.0.12.1: semantic diagnostic, hover,
completion, definition, correction and process cleanup pass. Lazy
`completionItem/resolve` returned the required GregorianCalendar import. No
completion callback/server command was executed. Exact provenance/checksum,
limits and evidence are in `LANGUAGE_SERVICES.md` and the language crate's
`tests/evidence` directory.

### Kotlin

Deprecated MIT-licensed fwcd server 1.3.13 (Kotlin compiler 2.1.0): actual type
error, typed hover, relevant completion, local definition, correction and zero
resulting diagnostics pass through the Rust language client. Shutdown required
forced process reaping and logged an upstream disposal error. This is not a
native Kotlin UI or current-Kotlin project compatibility pass.

The official 263.6379.0 archive was checksum-verified but requires an explicit
EULA and reports its bundled EULA.txt missing. No acceptance or semantic session
was attempted. Details and raw evidence: `KOTLIN_VALIDATION.md`.

### Debugging foundation

The separate cedar-debugger crate has 14 deterministic DAP process tests and a
real debugpy1.8.22 Python fixture: breakpoint, thread/stack/scope/answer=41 local,
resume/answer=42 output and termination. Graceful disconnect, adapter crash and
client drop were tested; no owned processes remained running in those test runs.

This is not an integrated GUI or remote debugger. Cleanup only generally
guarantees the direct adapter, and debugpy opens an additional unauthenticated
loopback client endpoint. No documented disable/authentication option was found.
These are recorded integration blockers, not silently waived. Java debugging is
not implemented. See `DEBUGGING.md`.

## Important fixes covered by regressions

- Workspace switch / save acknowledgement cannot replace newer drafts
- SSH response generations and LSP session/document/edit versions reject stale results
- Cancelled completion resolution cannot mutate the buffer later
- Full edit plans validate UTF-16 boundaries, CRLF, overlap, size, source snapshot and optional imports before atomic application
- Arbitrary server commands and workspace edits are not automatically executed
- Diagnostic versions after reopening a file cannot reuse old state
- Typing during asynchronous language shutdown triggers a fresh discard check
- Undo history is capped at 16 full-text snapshots per tab and released on close
- Search, tabs including pending opens, queues, protocol text, output and diagnostics have explicit bounds
- Git clean filters cannot bypass workspace execution trust
- Linux/macOS process-group signaling precedes reaping to avoid PID reuse
- URI navigation rejects external schemes, outside-root files, symlinks and Unix backslash aliasing

## Still unverified or missing

Real SSH authentication/network failure interoperability, Windows/macOS runtime,
IME/accessibility, production Maven/Gradle/Kotlin projects, official Kotlin
license/setup resolution, full debugger UI/remote adapter lifecycle, PTY,
crash-recovery storage, multi-file refactoring, plugin compatibility, signing and
deployment remain open. The phase-2 CJK-capable frontend-only sample measured 118.52 MiB HWM; Java/Kotlin JVM readings are reported separately. Memory samples and JVM experiments are separate,
small-fixture evidence; they do not establish IDEA-relative savings. See
`PERFORMANCE.md` and `FEATURE_MATRIX.md`.
