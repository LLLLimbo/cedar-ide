# Verification report · phase 4 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar **0.4.0**, frontend/agent protocol **4**, is an in-development Rust IDE
checkpoint, not a complete IntelliJ IDEA replacement. This phase adds explicit
single-document formatting previews, references and document outline. The final
local aggregate, Linux release build and final native-window regression checks
**passed**. Exact-commit phase-4 external CI remains pending. These passes do not
establish cross-platform GUI or production acceptance.

The [phase-3 report](TEST_REPORT_PHASE3.md) is preserved byte-for-byte against
tag `phase3-0.3.0`. The separate
[0.3.1 recovery hotfix report](TEST_REPORT_HOTFIX_0_3_1.md) is also retained as a
fixed historical record; its later CI result is recorded below.
[Phase 2](TEST_REPORT_PHASE2.md) and [phase 1](TEST_REPORT_PHASE1.md) likewise retain
their original scope; their measurements and real-server checks
are not automatically new phase-4 results.

## Current verification status

Environment: Linux x86_64, kernel 6.18.44, glibc 2.41, rustc 1.99.0.

| Check | Phase-4 status |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| Strict workspace Clippy, all targets/features and locked dependencies | PASS |
| Workspace tests, all targets/features | **336 ordinary tests passed**, zero failures, 4 opt-in tests ignored |
| Explicit client → separate-agent process test | **1 additional PASS**, one of the 4 aggregate ignored cases |
| Python filesystem/trust/conflict/EOF/malformed-frame smoke | PASS |
| Python agent → LSP lifecycle/completion/resolve/navigation/format-version/references/outline smoke | PASS |
| Python asynchronous task/live-output/edit/cancel smoke | PASS |
| Final `scripts/verify.sh` process | **PASS, exit 0** after the fixes described below |
| Frontend subset | 171 ordinary app tests passed; 2 opt-in tests ignored; included in the workspace total |
| Recovery subset | 37 passed (3 unit + 34 storage); included in the workspace total |
| Real JDT through actual stdio agent | **10 checks passed**; initial cold references timeout disclosed below |
| Native Linux candidate window | Preview, explicit Apply/Cancel, immediate undo/redo, outline/reference navigation and unchanged source disk verified on earlier debug candidate |
| Full phase-4 Linux release workspace build | **PASS, exit 0** |
| Phase-4 Windows MSVC full-workspace/all-target/all-feature compilation check | **PASS, exit 0**; compilation check, not phase-4 Windows execution |
| Final phase-4 Linux release native rerun | **PASS**: real JDT preview, Escape, Apply, cursor/tab navigation, single-step Undo/Redo and unchanged source disk |
| Public phase-3 CI | Windows recovery replacement test failed with AccessDenied; Linux matrix job was cancelled by fail-fast |
| Phase-3.1 corrective checkpoint | Published; local 249+1 tests passed; same-commit Linux/Windows CI **both passed**, including 29 Windows recovery tests, release build and stdio integration |
| Exact phase-4 external CI | Not yet established; the required prior phase-3.1 same-commit CI gate has passed |
| Windows/macOS native GUI acceptance | Not performed |

The initial failed aggregate log is
[verification-phase4-pre-fix-log.txt](../PUBLICATION.md#verification-evidence).
The final rerun is recorded separately in
[verification-phase4-log.txt](../PUBLICATION.md#verification-evidence), with confirmed exit 0.
Separate confirmed build logs: [release-phase4-build.txt](../PUBLICATION.md#verification-evidence)
and [windows-phase4-check.txt](../PUBLICATION.md#verification-evidence).
The first attempt records 163 app tests passing before three recovery failures:
`reopening_resets_only_process_sequence_tombstones`,
`round_trip_preserves_exact_unicode_and_original_revision`, and
`record_and_total_bounds_fail_without_eviction_or_false_acknowledgement`.
All failed with `Locked` on reopen. A concurrent fork can retain a duplicated
lock file descriptor before exec, even with close-on-exec set; closing the owner's
handle alone can therefore delay lock release. The candidate fix uses explicit
unlock on owner-process drop, with a process-identity guard so an inherited store
cannot unlock its living parent's lock. Focused verification of the Unix fix
passed **36 recovery tests and 30 stress runs**. The final full-workspace rerun
then passed. The failing log is retained rather than silently recategorized as
a passing run.

The final Rust count is **336 + 1 = 337 passed**, plus the three Python smokes.
Do not add app/backend/recovery subsets or the four ignored cases again. Real JDT
and system-font app checks and the real debugpy test remain opt-in; the separate
phase-4 JDT agent probe below does not mean all historical opt-in probes reran.
Headless layouts and app-frame input tests are not native-window acceptance.

## Implemented safety and regression coverage

### Plain TextEdit planner and formatting preview

The frontend accepts only plain `TextEdit[]`/null. It validates the complete
payload and plan before mutation, including exact field shapes, integer position
domains, UTF-16 surrogate boundaries, LF/CRLF/CR lines, nonoverlap and ambiguous
insertions. Limits are 1,024 edits and independently 1 MiB source, result and
aggregate replacement text. Cursor mapping uses Unicode-scalar offsets with
strict CRLF handling. Null, empty and text-identical results are no-ops without
an undo entry. See [TEXT_EDITS.md](TEXT_EDITS.md).

The agent checks the expected synchronized document version before sending a
format request. The UI captures connection, server session, request sequence,
unique document identity/path, source text and edit version. Before/After is
read-only; Apply checks the snapshot again and commits only the draft through
one native undo transaction. Project disk, saved baseline/revision and recovery
ownership are independent. Existing older unowned recovery records remain
protected. Separate save acknowledgements preserve newer typing.

Regressions cover stale replies and previews, edit/tab/close/reopen/reconnect
invalidation, double Apply, cancellation, all no-op forms, cursor-only movement,
exact synchronization versions, save-while-typing, filesystem/baseline nonmutation,
and older recovery preservation. Escape now reaches preview cancellation before
global shortcuts consume it; this is exercised through the actual app update.
The later cursor-only checkpoint fix passes real-frame regressions after cursor,
outline and tab navigation. Shortcut-only batches skip same-text checkpoints in
order; mixed text/paste/navigation batches pass unchanged to egui, preserving input
order. Tests cover Undo/Redo order, queued typing/paste, focus isolation and
bounded batches. Independent review passed. The final release native rerun below
separately confirms Escape and single-step undo after cursor/tab navigation.
Mixed-input ordering is verified by full-frame tests, not a native OS timing
simulation.

### References and document outline

Reference dispatch first synchronizes all matching open drafts and captures each
participant's identity, path, source, edit version and acknowledged LSP version.
Changed participants, a changed matching-open set or a moved source query position
invalidate pending responses. Retained results are explicitly an **unversioned
server snapshot**, without a freshness promise for unopened or later-modified
targets. Navigation resolves URIs on the agent, reuses dirty open buffers unchanged,
and validates ranges against current target text.

Hierarchical outline keeps class/method nesting and navigates via selectionRange;
flat SymbolInformation stays flat and displays container context. Mixed/hybrid,
malformed, oversized and excessively deep responses are rejected. Source ranges,
selection containment and child/parent containment are checked with strict
UTF-16/CRLF rules; unknown numeric kinds use a generic label. Edits invalidate
the outline. Counts/text are bounded as documented in
[PHASE4_LANGUAGE.md](../crates/app/PHASE4_LANGUAGE.md).

Tests cover provider guards, unavailable/open-document state, synchronized
references, changed participants, dirty targets, invalid positions, hierarchy/flat
parsing and 780×540 / 1320×880 headless layouts. Newer local outline selection now
invalidates an older pending URI resolution or file-open response so it cannot
steal focus; a cursor-only outline selection does not invalidate a document-only
formatting preview. General WorkspaceEdit, server commands, file resource
operations and live multi-file rename remain unsupported.

## Actual JDT through the workspace agent

`scripts/jdt_navigation_smoke.py` used an actual Cedar stdio agent and the installed
official JDT LS 1.61.0 milestone (server-reported `1.61.0-SNAPSHOT`) with a fresh
synthetic Eclipse Java project. This is an actual process/protocol chain, not a
mock peer or authenticated SSH session.

The passing capture records **10 checks** in **13.236 seconds**:

1. Initialized real JDT and observed formatting/references/document-symbol support
2. Synchronized two unsaved Java documents
3. Found one declaration and two calls, including both calls in the second unsaved
   buffer, with paths resolved through the agent
4. Returned an actual class outline with `greeting(String)` and `count()` methods
5. Returned 12 plain formatting edits, applied by the probe's UTF-16 oracle while
   retaining the Chinese comment and valid Java text
6. Rejected a stale format version with `language_stale_version`
7. Confirmed formatting of the synchronized result was idempotent
8. Rejected navigation outside the workspace root
9. Closed both documents and stopped the server
10. Verified original Java source bytes stayed unchanged and removed the fixture

Evidence: [jdt-navigation-phase4.json](../PUBLICATION.md#verification-evidence), including the
agent SHA-256, raw server results, source hashes and explicit absence of server
command or workspace/applyEdit execution. The Python oracle does not replace
Rust planner tests or native preview/Apply acceptance.

### Cold-start timeout and retry boundary

The first cold attempt failed when references reached the agent's **10-second
normal LSP request deadline**. A clean rerun produced the passing capture, which
has no readiness-timeout entries. The smoke script now explicitly permits retries
of that read-only query within a **40-second semantic-readiness window**. Its
outer per-agent-response limits are normally **20 seconds**, or **75 seconds for
startup**; the workspace's separate LSP initialization budget is **60 seconds**.
These outer limits do not increase the normal 10-second feature deadline.
Normal UI requests are not automatically retried. One clean rerun does not prove
reliable cold-start latency, production indexing behavior or a performance gain.

Reproduce using a separately installed, verified JDT distribution and Java runtime:

```sh
python3 scripts/jdt_navigation_smoke.py \
  target/debug/cedar-agent /absolute/path/to/jdtls \
  /absolute/path/to/jdt-navigation.json /absolute/path/to/java
```

## Native Linux window checks

### Earlier debug candidate

A real debug-binary OS window was operated with keyboard and pointer input on
Linux, using a configured local JDT wrapper and synthetic Java files. This
candidate predates the final Escape and late-navigation review fixes. It showed:

- A Chinese comment and compact one-line Java source in the native editor
- Read-only Before/After preview containing 12 real JDT formatting proposals
- Explicit Apply changing the draft while it remained unsaved
- Immediate Ctrl+Z and redo restoring the original and formatted text
- A hierarchical class/method outline and selection of the method declaration
- A reference list with two entries when including the declaration; selecting the
  Main.java call navigated there while the dirty Greeter draft remained intact
- Explicit Cancel leaving the clean unformatted draft unchanged
- Original Java source disk bytes remaining unchanged throughout

Session record: [native-language-phase4-qa.json](../PUBLICATION.md#verification-evidence).
Screenshots: [native-format-preview-phase4.png](../PUBLICATION.md#verification-evidence)
and [native-references-phase4.png](../PUBLICATION.md#verification-evidence).
This native fixture has one call plus its declaration, distinct from the agent
probe's two calls plus declaration. This earlier binary does not itself validate
the subsequent Escape, late-navigation or cursor-only-history fixes. The latter
fixes have full-frame/state regressions and independent review; the final release
window separately validates the flows below.

### Final reviewed release candidate

The rebuilt Linux release binary was operated in a real native window against
existing JDT LS 1.61.0 and a **warmed synthetic server-data directory**. The final
check passed:

- Real Java Before/After formatting preview preserved the Chinese comment
- Escape dismissed the visible preview and left the clean source unchanged
- A fresh preview followed by explicit Apply produced an unsaved formatted draft
- After moving the cursor, opening Main.java, switching back to Greeter.java and
  refocusing its editor, **one Ctrl+Z** restored the entire unformatted source
- **One Ctrl+Shift+Z** restored the whole format; another Undo returned the buffer
  to its clean source baseline
- Both Java source files on disk remained byte-identical throughout

Evidence: [native-release-phase4-qa.json](../PUBLICATION.md#verification-evidence) and
[native-release-phase4.png](../PUBLICATION.md#verification-evidence).
The tested frontend binary SHA-256 is
`6c529ee30f4981eb2389c4287c06b35c1145ee0a6394f6dc4500ddb6ccc75b92`.
This is native acceptance of the listed final-release flows. It does not claim a
new cold-start run, new performance measurement, Windows GUI acceptance or a
native OS mixed-input timing simulation; the latter remains automated full-frame
coverage. Late reference navigation losing to a newer outline click is also
covered by app-update/state regressions rather than this manual sequence.

## Public source and platform evidence

The public source repository is [LLLLimbo/cedar-ide](https://github.com/LLLLimbo/cedar-ide).
The first three source-only stage commits were published with the phase-3 source at
[`01d616cdf969691339ecc376c5228b10760077f0`](https://github.com/LLLLimbo/cedar-ide/commit/01d616cdf969691339ecc376c5228b10760077f0).
They omit machine-specific raw logs/screenshots, while retaining source, licenses
and the deterministic compile-time fixture. Public export hashes differ from
private build/evidence commits.

[Phase-1 CI run 37624801583](https://github.com/LLLLimbo/cedar-ide/actions/runs/37624801583)
passed Linux and Windows jobs. This is useful core build/test evidence for that
exact historical source; it does not verify the latest phase-4 commit. The later
[public phase-3 Windows job](https://github.com/LLLLimbo/cedar-ide/actions/runs/37625344235/job/112805556824)
failed a recovery replacement test with `AccessDenied`; its Linux matrix job was cancelled by fail-fast and
must not be counted as either a code failure or a passing run. This exposed a
concurrent-reader replacement path, not merely a cross-compilation warning. A
separate minimal phase-3.1 correction adds held-reader and deny-sharing
regressions, without weakening the concurrent-reader assertions. On Windows the fix uses Rust 1.99
`std::fs::rename` for compatible open-reader replacement; restrictive sharing
still fails visibly without deleting the prior record or falsely acknowledging
backup. There is no delete-then-rename gap or blanket access-error retry. Details
and platform limits are in [RECOVERY.md](RECOVERY.md).

The separate hotfix checkpoint passed **249 ordinary tests plus 1 explicit
agent test** locally, along with a release build and Windows target compilation
check. These are not phase-4 totals or native Windows acceptance. The corrective
public commit is
[`9f9a72608d7ea944d5946656f8fdf259a950891d`](https://github.com/LLLLimbo/cedar-ide/commit/9f9a72608d7ea944d5946656f8fdf259a950891d),
with the complete 621-file source tree verified. Its
[CI run 37627832872](https://github.com/LLLLimbo/cedar-ide/actions/runs/37627832872)
completed with **both Linux and Windows successful for that same SHA**. Windows
ran **29 recovery tests**, including concurrent replacement, held-reader behavior
and restrictive deny-sharing, then passed the release build and separate-agent
stdio integration. This is real Windows core runtime evidence for the hotfix,
not just a target compilation check. The same-SHA gate before phase-4 publication
is satisfied. A separate hotfix release-window recovery check is not phase-4
release acceptance.

Windows core tests and release builds in CI are **not native GUI, IME,
interactive recovery, language-server or command acceptance**. Windows local
tool execution remains disabled pending Job Objects and cancellable pipes.
macOS native behavior remains unverified.

## Historical evidence and remaining gaps

- Phase 3 verified acknowledged Chinese draft recovery after process kill, conflict
  rejection after external disk edits, asynchronous commands while editing/saving,
  and explicit cancellation. See [TEST_REPORT_PHASE3.md](TEST_REPORT_PHASE3.md)
- Its 112.597-second release frontend sample observed HWM 117.97 MiB. That is a
  historical direct-PID sample, not a new phase-4 measurement or an overhead claim
- Phase 2 verified JDT completion/import/diagnostics/navigation in the native UI,
  system CJK fonts, an older deprecated community Kotlin server and separate real
  debugpy transport. Official Kotlin license/setup and integrated debugging remain
  unresolved; see [TEST_REPORT_PHASE2.md](TEST_REPORT_PHASE2.md)
- No simultaneous frontend/JVM total, IDEA comparison, production benchmark or
  sustained leak test was added. See [PERFORMANCE.md](PERFORMANCE.md)
- Authenticated SSH/network-failure interoperability, task outcome reconciliation,
  Windows/macOS native acceptance, power-loss durability and production project
  import remain unverified
- Multi-file rename needs trustworthy snapshots, resource-operation completeness
  and cross-document undo. General workspace edits, code actions, snippets,
  comprehensive project models, integrated debugger UI/agent, PTY, test tree,
  complete Git workflows, stable plugins and signed distribution remain absent

Current implementation boundaries and the remaining sequence are documented in
[FEATURE_MATRIX.md](FEATURE_MATRIX.md) and
[REFACTORING_ROADMAP.md](REFACTORING_ROADMAP.md).
