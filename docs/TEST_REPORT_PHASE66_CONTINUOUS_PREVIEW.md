# 0.66 continuous Project Text preview and repaint correction history

## Accepted 0.66.2 bounded baseline

Public commit
[`a0ecbdbe27577ef83a60eee1b8af88edb0c68f35`](https://github.com/LLLLimbo/cedar-ide/commit/a0ecbdbe27577ef83a60eee1b8af88edb0c68f35)
passed all **four jobs** in
[CI 38392603729](https://github.com/LLLLimbo/cedar-ide/actions/runs/38392603729)
and all **three exact-source package audits**. Linux recorded **2,128/0/70** and
Windows **2,066/0/114** (passed/failed/ignored), each across **50 suites**, including
all **17 preview tests** on both systems. Strict Browse passed **v4, 50 fields /
14 operations**, retaining all 38 prior field assertions and the original
12-operation prefix. Scoped Search **ten operations**, Search **twelve
operations** and both platform JDT proofs were verified.

The fresh bounded Linux native journey passed continuous follow in both Text
surfaces, the previously failed bottom B160 preview, ordinary Open/Back, dirty
selection and Undo/Redo, and Escape/hide/reopen. Startup was **14.020 seconds**;
quit began **491.046 seconds after visible Connected** and exit code **0** arrived
**17.309 seconds** later. The bounded functional and timing gates passed.
Native pixels do not prove the **150 ms** settlement interval, request ledgers or
raw caret affinities; those remain separate deterministic/process evidence.
Windows/macOS GUI, real SSH, arbitrary viewports and broad CJK/IME remain outside
this acceptance.

0.66.2 changes the test-only caller helper to recognize the exact `browse.rs`
basename with either path separator. The 0.66.1 production repaint implementation
is byte-for-byte unchanged. The fresh Windows results establish the portability
correction’s pass without rewriting the earlier failure’s unlogged runtime cause.
The 0.66.0 native bottom-preview failure, 0.66.1 Windows caller-count failure and
never-launched native check, 0.65.0 glyph stop and 0.64.0 strict timing failure
remain preserved below and in their phase reports.

## Preserved 0.66.2 preparation checks

The checks in this section preceded the exact CI, package and native acceptance
above. They remain separate evidence rather than claims about later candidates.

The test-only helper recognizes an exact `browse.rs` basename with either slash
or backslash path separators, independently of the host platform. It must not
accept a different filename merely containing that text. Synthetic cases cover
relative, absolute, bare and mixed-separator names, reject near matches, and
retain separate counts for multiple matching callers. The original exact
application-repaint count assertions and the old-production negative control
remain required; platform portability does not relax them. Bounded diagnostics
report a fixed basename/separator histogram and completed-pass count without raw
caller paths. No production behavior, repaint timing, deadline, dependency or
backend operation changes.

The current focused Rust checks passed **17/17**, including the retained 16
preview cases and the portable exact-basename test. The old slash-only predicate
failed a controlled Windows-style path case as intended. With the corrected
tests and exact old 0.66.0 production code, both Text surfaces still failed the
missing-wake assertion: internal B160 was ready while the painted state remained
pending, with no scheduled repaint instead of the required zero-delay repaint.
The current production and test bytes were restored before the positive run.
These negative controls establish test sensitivity; the actual Windows result is recorded separately above.

The current workspace/all-target/all-feature/locked Rust aggregate passed
**2,128 tests / zero failures / 70 ignored across 50 suites**. The Python
aggregate passed **793 total tests / zero failures / 10 skipped across 28 suites**.
Independent review confirmed the test-only scope and production-byte equivalence.
Strict workspace/all-target/all-feature Clippy with warnings denied and formatting
passed.

The current Cargo-JSON-selected app and matching default-features normal agent
passed strict Browse **v4, exactly 50 fields / 14 operations**, retaining all
**38 prior field assertions** and the original **12-operation ledger prefix**.
The retained scoped-search proof passed its strict **ten-operation** validator.
The canonical release compile/acceptance drivers were **not run locally**;
these current-artifact proofs do not replace the later exact CI, package or native
gates.

All three current packaged-guide link-closure checks passed with Python's `-B`
option: Windows bundle, Linux agent bundle and Linux desktop bundle. They validate
nested local-link targets without building packages or establishing app/native
behavior. The documentation diff passed `git diff --check`.

## Preserved 0.66.1 CI failure and partial package results

Public commit
[`f972998b212889a6cb8d2e900e2c077a9c3af165`](https://github.com/LLLLimbo/cedar-ide/commit/f972998b212889a6cb8d2e900e2c077a9c3af165)
ran in
[CI 38388794011](https://github.com/LLLLimbo/cedar-ide/actions/runs/38388794011).
Three new Windows repaint tests failed because the application-caller count was
**zero instead of one**, despite a zero-delay due-frame repaint and valid
internal B160 preview state. The raw repaint-cause filename was not logged.
The source-level explanation is a path-separator mismatch between the test's
hard-coded `/browse.rs` suffix and the platform path in pinned egui's
`Location.file`; this remains a hypothesis, not a runtime-confirmed root cause.
That failure could not establish a Windows fix; the fresh 0.66.2 Windows pass is recorded separately above.

The Linux aggregate passed **2,127/0/70 across 50 suites** (passed/failed/ignored),
including all **16 preview tests**. Strict Browse passed **50 fields / 14
operations**, retained proofs passed, and both Linux packages were verified.
The Windows app suite recorded **1,411 passed / three failed / 57 ignored**;
the observed aggregate prefix was **1,421/3/57 across eight suites**, not a
complete Windows aggregate. Preview coverage was **13 passed / three failed**.
Windows release/runtime proofs and its package were absent. The dependent Java
signature matrix was entirely skipped; neither platform producer job executed.

The Linux Java proof matched a spontaneous diagnostic without recovery. Its
restart cleanup used `grace_expired` / **SIGKILL (signal 9)** under the existing
cleanup predicate, so that restart was not graceful. This does not replace the
skipped signature-matrix producers.

**No 0.66.1 native run was launched.** It remained held at the Windows CI gate.
The two Linux package results and earlier local passes do not override that gate.

## Preserved 0.66.1 local repaint correction checks

The 0.66.1 correction followed the 0.66.0 native failure below. Independent
review, focused and full local checks, strict Clippy, formatting and
current-artifact proofs passed locally; those results do not validate 0.66.2.

A separate controlled actual-frame diagnostic reproduced a missing repaint on
both Text surfaces. At the accepted 150 ms settlement deadline, fulfillment from
an open draft happened after that frame's preview had been painted. The internal
preview already held the selected B160 context, but the frame still contained
“Preview requested...” and scheduled no further repaint. This establishes a
source/frame defect; the earlier native observation did not expose this internal
state or scheduling trace and does not establish that exact native cause. The
controlled regression checks failed as intended on the old production source,
covering both surfaces before the correction.

The correction requests one repaint only when authorized due fulfillment
synchronously produces a visible Draft or Message preview. Asynchronous Read
dispatch keeps its existing wake behavior. The fixed 150 ms deadline,
one-flight/latest-candidate behavior, zero-Read draft reuse, focus and
stale-identity boundaries remain. The correction adds no polling loop, timing
delay or new backend operation.

The final-version focused Rust checks passed **16/16**, comprising **11 retained
cases and five new repaint regressions**. On both surfaces, due open-draft
fulfillment requests one application repaint and the next scheduled frame paints
B160 context. The framework's settling frames drain back to idle. These checks
also retain dirty text, selection, Undo, history, result focus and zero Read, and
cover synchronous refusal/request exhaustion, due Escape/target changes and a
canceled late reply that cannot restart preview.

The final-version workspace/all-target/all-feature/locked Rust aggregate passed
**2,127 tests / zero failures / 70 ignored across 50 suites**. The Python aggregate
passed **793 total tests / zero failures / 10 skipped across 28 suites**. Strict
workspace/all-target/all-feature Clippy with warnings denied and formatting
passed. Independent review cleared the correction.

The final current app selected from Cargo JSON and the matching default-features
normal agent passed strict Browse **v4, exactly 50 fields / 14 operations**,
retaining all **38 prior field assertions** and the original **12-operation
ledger prefix**. The retained scoped-search proof passed its strict
**ten-operation** validator. Existing source-hash and cleanup checks remain.
The canonical release compile/acceptance drivers were **not run locally**;
these current-artifact proofs do not replace exact CI, package or native gates.

The first current-artifact scoped-search attempt stopped on stale 0.66.0 version
expectations in process fixtures. Four fixture literals were updated for 0.66.1
before sealing, without changing feature behavior or weakening validation. The
failed attempt remains recorded. The final full/strict checks and both runtime
proofs above passed after that maintenance.

The three current packaged-guide link-closure tests passed for the correction's
documentation with Python's `-B` option: Windows bundle, Linux agent bundle and
Linux desktop bundle. These checks validate nested local-link targets; they do
not build packages or validate app/native behavior. `git diff --check` passed.

## Preserved 0.66.0 CI, packages and stopped native journey

Public commit
[`84d7393f3ace6ef153bed26994f480ccd9bf1abe`](https://github.com/LLLLimbo/cedar-ide/commit/84d7393f3ace6ef153bed26994f480ccd9bf1abe)
passed all **four jobs** in
[CI 38382019936](https://github.com/LLLLimbo/cedar-ide/actions/runs/38382019936)
and all **three exact-source package audits**. Each platform ran **50 suites**:
Linux **2,122 passed / zero failed / 70 ignored** and Windows **2,060 passed /
zero failed / 114 ignored**. Strict Browse retained **version 4, exactly 50
fields / 14 operations**, including all **38 prior field assertions** and the
original **12-operation ledger prefix**. These results belong to 0.66.0 and do
not validate either correction candidate.

The bounded Linux native journey **failed its essential functional gate**.
In bottom FIND RESULTS, Down selected the already-open `src/层/B.rs` historical
line 160 after an armed A150 preview. The pane still displayed
“Preview requested...” at the separate observation and again **37.223 seconds**
after Down, immediately before quit, with no intervening functional input.
No recovery or repeated selection was attempted. Bottom Up and bottom
Escape/hide/reopen checks were **not run**.

The earlier modal Project Text journey passed explicit arming, continuous Up/Down
follow, ordinary same-row double-click Open, Back to the selected dirty draft,
Undo/Redo, reopening without rearming, fresh Space and Escape disarming. The
observed CJK result paths were readable. Those passed steps do not turn the
stopped native journey into a pass, and pixels do not establish exact 150 ms
settlement or Read counts.

Startup was **16.444 seconds** against 90 seconds. Safe quit began
**348.689 seconds after visible Connected**, before the 550-second wind-down and
600-second case limits. Exit code **0** arrived **9.907 seconds** after quit,
and the window was absent at **10.027 seconds**, within the 120-second shutdown
bound. The observed timing, cleanup and integrity gates passed; source, package
and fixture contents were unchanged. The functional failure remains recorded.

The Linux Java proof first timed out waiting **60 seconds** for a spontaneous
diagnostic, then matched after the existing allowed recovery refresh. Its
passing recovery path is not a spontaneous-diagnostic pass. The initial Java
stop was graceful; restart cleanup used `grace_expired` / **SIGKILL (signal 9)**.
Satisfying the cleanup predicate does not mean both Java stops were graceful.

The preparation results below belong to **0.66.0**. They preserve earlier failed
attempts and do not establish acceptance of the later candidates.

## Candidate contract

- Scope is only **Project Text in Find and return** and bottom **FIND RESULTS**.
  Files keeps its current explicit-preview interaction.
- An explicit current owned **Space / Preview** action arms the exact Text surface
  and accepted result-view identity. First preview is immediate and keeps result
  row focus. Unarmed selection, hover and query typing send zero preview Reads.
- Later real owned selection changes settle for a fixed **150 ms** before preview
  admission. Repainting cannot renew the selection's deadline. Retain at most one
  preview Read in flight and one latest replacement candidate; superseded rows
  are not a queue. Stale success/error cannot replace current preview or focus.
- Open source drafts, including dirty drafts, supply their current bytes with
  **zero Read** and preserve text, full selection and Undo. Unopened paths use
  the existing bounded Read and workspace boundary.
- Show at most **41 lines / 64 KiB** of line context, clipping only at valid UTF-8
  boundaries with truncation visible. The indicated line is a **historical line
  hint**, not a verified current match. A historical line past current EOF is
  unavailable, not clamped. The existing source Read limit stays **1 MiB**.
- Escape, hide/dismiss, query/filter edits, rerun, provider/surface changes, bound
  document-version changes, navigation and reconnect disarm and retire intent.
  Reopening needs a fresh Space / Preview action; late replies cannot reopen a
  hidden preview, resume automatic Reads or restore old focus.
- Ordinary Open/Back and genuine same-row double-click retain their current
  captured-identity and draft-preservation contracts. Preview adds no editor tab,
  Save, index, persistence, protocol, backend operation or service.

User-facing detail lives in the existing packaged
[Find guide](FIND_AND_RETURN.md#continuous-project-text-preview-066-candidate).
That guide links to this report through an external repository URL, preserving
its offline package closure without adding a new packaged guide dependency.

## Preserved 0.66.0 documentation checks

The three current packaged-guide link-closure tests passed with Python's `-B`
option: Windows bundle, Linux agent bundle and Linux desktop bundle. The checks
read the current guide bytes and validate their nested local-link targets.
No package was built or audited, and no app/native validation was performed by
these documentation checks. The documentation diff also passed `git diff --check`.

## Preserved 0.66.0 local focused and Python checks

The current focused checks passed **11 new actual-frame/context tests**, the
**59 retained Browse tests** (with one process test ignored in that focused
invocation), and **43 Search tests**. The Python aggregate recorded **793 total
/ zero failures / 10 skipped across 28 suites**. Independent source review
cleared the current changes. After the corrections recorded below, the final
Linux Rust aggregate passed **2,122 tests / zero failures / 70 ignored across
50 suites**. Final strict Clippy and formatting checks passed.

The focused coverage addresses the candidate's selection settlement, exact-view
arming, one-flight/latest-candidate behavior, unarmed and dirty-source zero-Read
paths, UTF-8/line/truncation/EOF boundaries, stale work and disarming, row focus,
Files controls, double-click and ordinary Open/Back boundaries. These are local
checks; they do not establish native GUI behavior or exact-release acceptance.

## Preserved 0.66.0 current-artifact normal-agent proof

The final current app artifact selected from Cargo JSON and the matching normal
agent built with default features passed the strict Browse validator with
**version 4, exactly 50 fields and 14 operations**. All **38 pre-existing field
names and assertions** and the original **12-operation ledger prefix** are
preserved. The existing source-hash and cleanup assertions remain in place.
The retained scoped-search proof also passed its strict **ten-operation**
validator with these current artifacts.

These are direct current-artifact runtime proofs, not comparative performance
measurements. The canonical release compile/acceptance drivers were **not run
locally**; they remain exact-CI gates. These local proofs do not claim that the
canonical release drivers or package/native checks have passed.

## Preserved local fixture failures

The first new process attempt failed because location history coalesced an
identical top entry. A second attempt added a distinct departure precondition,
but that precondition failed: the surviving editor was active without keyboard
focus. The corrected proof first clicks the editor's current rendered rectangle
and verifies its focused widget identity before setting the keyboard departure.
It preserves the original **one added history entry** and **Back restoration**
assertions. This was a test-only input correction, with no production change or
extra Read; the earlier failed attempts remain recorded.

Earlier focused attempts also exposed test-helper compilation errors and
fixture assumptions about global triple-click input and a historical line 2 in
a one-line source. These failures remain part of the preparation record. The
past-EOF contract continues to require unavailable context rather than clamping;
the final passing checks do not erase those failed attempts.

## Preserved first Rust aggregate failure

The first full Rust aggregate failed one pre-existing scoped-geometry assertion;
the app suite recorded **1,405 passed / one failed / 53 ignored**. That assertion
expected ArrowDown in bottom FIND RESULTS to clear preview. The approved 0.66
behavior intentionally keeps continuous preview armed and updates it to the
settled selected row, so this is an intentional contract change, not merely a
fixture-only correction or a production bug fix.

The narrow test update now requires the newly selected row's settled historical
context, a visible preview pane and result row, and zero Read for the open source.
It then uses real Escape to verify disarming. The original ordinary/narrow
geometry cases at **13 and 18** font sizes remain. The failed first aggregate
record is preserved. The second full Rust aggregate passed **2,122 tests / zero
failures / 70 ignored across 50 suites**.

The subsequent strict check found only a boolean `assert_eq!(..., true)` style
issue and formatting wraps. Those test syntax/formatting corrections changed no
production behavior. A final full aggregate, strict Clippy, formatting and
current-artifact Browse/scoped-search proof recheck after those edits all passed.
The final aggregate again recorded **2,122/0/70 across 50 suites**. The earlier
failed strict check and aggregate remain in the record.

## Acceptance boundary

0.66.2 completed independent review, focused Rust checks, both negative controls,
full Rust/Python aggregates, strict Clippy, formatting, current-artifact proofs,
exact dual-platform CI, three package audits and its bounded Linux native
journey. The failed 0.66.0 native step and 0.66.1 Windows tests remain historical
failures; the 0.66.1 native run was never launched. They are not relabelled as
0.66.2 evidence.

The accepted native evidence belongs only to that exact package and its observed
journeys. It does not validate the new File Structure candidate. Windows/macOS
GUI, real SSH, arbitrary viewports, broad CJK/IME coverage, IDEA parity and
comparative performance remain unestablished.
