# 0.66 continuous Project Text preview candidate

**This candidate is not yet accepted.** The latest accepted bounded baseline is
[0.65.1](TEST_REPORT_PHASE65_PROJECT_REVEAL.md#accepted-0651-bounded-baseline).
The 0.65.0 native glyph stop and 0.64.0 strict native timing failure remain
preserved in their reports. None of those prior results validates this change.

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

## Documentation checks

The three current packaged-guide link-closure tests passed with Python's `-B`
option: Windows bundle, Linux agent bundle and Linux desktop bundle. The checks
read the current guide bytes and validate their nested local-link targets.
No package was built or audited, and no app/native validation was performed by
these documentation checks. The documentation diff also passed `git diff --check`.

## Local focused and Python checks

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

## Current-artifact normal-agent proof

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

## Remaining validation

Local focused, aggregate, strict and current-artifact checks above passed.
Fresh exact CI, all three package audits and native validation have **not run**. Their results
must be recorded separately when available. Existing 0.65.1 receipts cannot be
relabelled as 0.66 evidence.

Native evidence must be bound to the new exact package and limited to observed
journeys. Windows/macOS GUI, real SSH, arbitrary viewports, broad CJK/IME coverage,
IDEA parity and comparative performance remain unestablished.
