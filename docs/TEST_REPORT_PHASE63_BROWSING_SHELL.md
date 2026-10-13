# 0.63.2 browsing shell pointer lifecycle correction candidate

**Acceptance pending. The latest accepted bounded baseline remains 0.62.2.**
Its exact public source, CI, package and limited native evidence are recorded in
[the accepted baseline](TEST_REPORT.md#accepted-0622-bounded-scoped-search-baseline).
The preceding 0.63.0 and 0.63.1 checks and incomplete native attempts remain
separate in the [current verification report](TEST_REPORT.md).

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

## Pointer lifecycle correction

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

## Evidence and limits

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
Browse result are excluded from current-candidate evidence. Both proofs then
passed with the exact current package artifact selected from Cargo JSON. No source
predicate was changed to bypass the version check.

Fresh exact-commit dual-platform CI, all three exact-source packages and the
bounded Linux native journey remain required. Earlier package or native evidence
is not a pass for this new shipping change.

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

The next native gate must observe the exact new package with execution trust off,
menu Escape, Project/Find/editor focus and retention, explicit Preview, genuine
same-row Open, Back and dirty Undo/Redo, followed by safe discard/quit and file
integrity checks. Native event delivery, raw affinity and backend operation counts
cannot be inferred from pixels. Windows GUI, real SSH, full IDEA parity and
comparative performance remain unverified.
