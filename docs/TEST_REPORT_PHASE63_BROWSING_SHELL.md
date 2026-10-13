# 0.63.3 browsing shell — bounded acceptance

## Accepted checkpoint

Public [0.63.3 source](https://github.com/LLLLimbo/cedar-ide/commit/5b6af6c51bfa8e460fc4b4b6367d09d744b1df78)
passed all four jobs in
[CI 38346482830](https://github.com/LLLLimbo/cedar-ide/actions/runs/38346482830)
and all three exact-source package audits. Each platform ran 50 suites: Linux
**2,063/0/70**, Windows **2,001/0/114** (passed/failed/ignored).

A fresh bounded Linux native journey observed functional Project Tree focus
after Alt+1 with the dirty draft intact and no inserted `1`, retained Find state,
Ctrl+J transitions, explicit Preview, ordinary Open, Back, Undo and both menu
Escape cases. This is separate from the headless tests and earlier native
attempts. Windows/macOS GUI, real SSH and arbitrary viewport behavior remain
unverified. The [current report](TEST_REPORT.md#accepted-0633-bounded-browsing-shell-baseline)
records the accepted scope and preserves earlier evidence limitations.

## Preserved preparation record

The following preparation and then-pending statements predate that accepted
checkpoint. They do not revoke the bounded acceptance or establish a result for
the separate [0.64 Recent Files candidate](TEST_REPORT_PHASE64_RECENT_FILES.md).

**Acceptance pending. The latest accepted bounded baseline remains 0.62.2.**
Its exact public source, CI, package and limited native evidence are recorded in
[the accepted baseline](TEST_REPORT.md#accepted-0622-bounded-scoped-search-baseline).
The preceding 0.63.0, 0.63.1 and 0.63.2 checks and incomplete native attempts remain
separate in the [current verification report](TEST_REPORT.md).

## Alt+1 companion correction

The 0.63.3 candidate is limited to the exact clean shortcut sequence
Alt+Num1 followed by one `Text("1")` event. The matching companion must be consumed
when the Project shortcut passes its admission checks, before ordinary editor
handling. Only final guarded success in focusing a current enabled Project
control may arm a fence for repeats of that same held key. A later focus failure
must not arm it; release and repress cannot reuse it.

This is not a general Alt-text filter. Unrelated text, multiple Text events,
Paste, IME, AltGr, mixed actions and stale ownership retain their rejection rules
and preserve unowned content events. The key-only route, Space and Ctrl/Cmd+J are unchanged. No
diagnostic observer is part of the shipping correction.

## Final 0.63.3 local validation

The final Linux workspace/all-target/all-feature/locked aggregate passed
**50 suites: 2,063 passed / zero failed / 70 ignored**; app **1,347/0/53**.
The focused Access selection passed **40 tests**, comprising six new Alt+1
test functions and 34 retained Access/Space tests. Formatting, strict
workspace/all-target/all-feature Clippy with warnings denied, and **779 Python
tests across 28 suites** passed.

The new positive actual-frame cases cover Tree and Flat modes with Find hidden
and visible. They retain the dirty full selection, Undo and location history;
Tree Down/Up provides a functional Project-focus witness without added operations.
Negative cases exercise exact event ordering and Text matching, rejection with
unowned content events retained, held repeats, release, blocked admission and final
focus invalidation. The supported root Search fixture establishes retained query
and result state, not execution of scoped search.

All six new functions initially failed while preparing a scoped-search fixture
that did not advertise the required capability, before reaching correction
assertions. The fixture was changed to supported root Search. The initial setup
failure remains recorded; it is not a production-assertion failure or evidence
that the new tests exercise scoped-search execution.

Current app and agent executables passed the retained direct normal-agent Browse
and scoped-search proofs through their unchanged strict validators, with exact
**seven/ten operations** and **zero added shell-transition or movement operations**.
The app executable was selected from the current Cargo JSON build output. These
are direct local process checks; the canonical release/full-CI driver was not
run locally. New exact-commit CI, all three exact-source package audits and
bounded native acceptance remain pending.

## Connected browsing scope

Project, the editor and Find are the main connected workspace. Workspace groups
connection/setup, recovery and save controls; Tools retains Git, Commands,
Language and Tests. Error, recovery, connection and dirty states remain visible
or directly reachable. Back/Forward uses the existing location history in the
editor tab band. No execution behavior or trust permission changes.

Alt+1 focuses a useful enabled Project item without hiding Find or issuing a
request. Ctrl/Cmd+J returns to the captured editor when hiding tools; reopening
Find targets an eligible current visible selected hit or the query field.
Historical, hidden, collapsed, clipped or stale results do not gain authority.
Find retains explicit submission, scoped literal search, grouped retained results,
single-click selection, explicit Preview and guarded same-row double-click Open.
See [Find and return](FIND_AND_RETURN.md) and [scoped search](SCOPED_PROJECT_TEXT.md).

The retained 0.63.1 Escape correction closes only a proven current Workspace or
Tools menu at the existing generic dismissal branch. Earlier modal/language
owners keep priority. Missing or stale menu identity cannot close another root.

## Retained 0.63.2 pointer lifecycle correction

The 0.63.2 change uses a separate pointer eligibility view that excludes only
IME Enabled/Disabled lifecycle notifications. It applies to actual pointer
press/release classification and an already owned held gesture. It does not
consume, replace or replay input. The original full event view still controls
keyboard shortcuts and Enter/Space button activation.

An active composition at the start of the batch remains a veto, even when that
batch disables it. Preedit and Commit, including empty content, remain competing
input, as do typing, paste, keys, modifiers and other pointer ownership. Lifecycle
notifications cannot repair a poisoned press. Exact document/session/view,
original-batch, response ownership, pass and final focus guards are unchanged.
This follows the existing Search and replacement pointer policy; it does not
claim broad IME correctness or identify the prior native event sequence.

## Preserved 0.63.2 deterministic evidence

A clean actual-frame reproduction of dirty selection, scoped two-hit search,
modal Escape and an isolated Find Results pointer click passed on unchanged
0.63.1 shipping code. A separate six-scenario characterization found clean and
MouseMoved clicks admitted, but standalone Enabled/Disabled, Preedit and Commit
notifications rejected despite a genuine egui click. Every scenario preserved
text, full selection, Undo, location history and result identity with no added
operation. This established a classifier limitation, not the native cause.

The corrected focused selection passed **47 tests**. It includes the clean and
motion cases, all six Enabled/Disabled press/held/release placements, fifteen
content/composition/poisoned/held-key journeys, three raw-input classifier tests,
and retained shell/access behavior. Positive journeys require actual click
admission, a visible bottom Find and exact current-row focus. Negative journeys
require no activation and unchanged draft, selection, Undo, history and results.
Raw-input tests retain atomic pointer handling, keyboard strictness and mixed
input rejection. The original rejection characterization remains preserved.

The final local Linux workspace/all-target/all-feature/locked aggregate passed
**50 suites: 2,057 passed / zero failed / 70 ignored**; app **1,341/0/53**.
Strict workspace/all-target/all-feature Clippy passed with warnings denied.
Formatting and **779 Python tests across 28 suites** passed. The existing strict
Browse and scoped-search runtime validators accepted direct normal-agent proofs
with exactly **seven/ten operations** and **zero shell-transition operations**.
Those proofs used the compiled app test and debug agent through a bounded runner;
the canonical CI compile/selection/release-driver route was not run locally.
They establish their own process results, not release packages or dual-platform CI.
An earlier direct invocation accidentally paired an older app-test executable with
the new agent. The scoped version guard rejected it; that failure and the earlier
Browse result are excluded from 0.63.2 evidence. Both proofs then
passed with the exact 0.63.2 package artifact selected from Cargo JSON. No source
predicate was changed to bypass the version check.

The subsequent public
[0.63.2 source](https://github.com/LLLLimbo/cedar-ide/commit/7850e557b6aaff647d1e71e31b1e239d271655d4)
passed all four jobs in
[CI 38338899789](https://github.com/LLLLimbo/cedar-ide/actions/runs/38338899789)
and all three exact-source package audits. Each platform ran **50 suites**:
Linux **2,057/0/70**, Windows **1,995/0/114**. Native acceptance remained incomplete.
Those results are evidence for 0.63.2 only, not a pass for the 0.63.3 change.

## Fresh local diagnostic and its limits

A separate instrumented Linux diagnostic recorded delivered Alt+Num1 plus a
Text-kind event, no admitted shortcut and an editor version change from 5 to 6.
The selected generated draft visibly became `1`; the raw Text payload was
intentionally not logged. The pinned input adapter permits printable Text with
Alt on this Linux path, establishing a relevant mechanism in this fresh
observation. This does not prove that earlier native runs delivered the same
event batches.

The diagnostic had Tools closed, omitting the planned Find baseline. Down/Up and
Ctrl+J were not run. Its complete loss-free trace recorded zero post-baseline
dispatches. The last functional action preceded the 300-second deadline by
23.654 seconds; quit was requested 66 milliseconds after that deadline and owned
exit followed 18.122056 seconds later. The **55 diagnostic tests** and successful
build do not establish shipping acceptance or excuse that timing overrun.

## Geometry and native acceptance boundary

Frame checks measure Cedar's connected header at **40/48 logical pixels** at
**780 × 540** and **1280 × 900**, with **13/18** fonts. The **28/34** tools values
are title-row minima, excluding margins, separator and body. Wrapping, scroll and
Automatic height preserve reachability; these are not hard tiny-window caps or
IDEA pixel-equivalence claims.

The 0.63.1 desktop-input native journey connected and passed both menu Escape
cases, filename discovery/open, a dirty draft, scoped two-hit search and selection
restoration after modal Escape. Find Results did not visibly open its panel after
a later isolated click, so the remaining focus/navigation cases were not run.
Earlier window-targeted connection attempts and their observation limits remain
separate. A subsequent successful route does not explain those failures.

The 0.63.2 exact-package journey observed connection with trust off, both menu
Escape cases, Find Results header opening, filename discovery/Open, a dirty draft
and scoped two-hit search. Alt+1 preserved Find state, but Flat mode provided no
functional Project-focus witness. It stopped as ambiguous, not as a proven
product failure. Ctrl+J, Preview, double-click Open, Back and Undo were not run.
Normal exit returned 0 and checked fixture, package and source bytes were intact.
Some original 0.63.1 prelaunch files had already been overwritten; surviving
duplicate image bytes do not reconstruct them. The separate later records remain
intact. See the [verification report](TEST_REPORT.md) for the preserved boundaries.

The next native gate must observe the exact new package with execution trust off,
menu Escape, Project/Find/editor focus and retention, an intact selected dirty
draft after Alt+1, explicit Preview, genuine same-row Open, Back and dirty
Undo/Redo, followed by safe discard/quit and file integrity checks. Project focus
needs a visible functional witness, not an inference from an unchanged Flat-mode
image. Native event delivery, raw affinity and backend operation counts cannot
be inferred from pixels. Windows GUI, real SSH, full IDEA parity and
comparative performance remain unverified.
