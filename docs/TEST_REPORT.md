# Verification report · Open-buffer location history / 0.31.0

This checkpoint implements bounded Back/Forward editor navigation using existing
open buffers. The prior [0.30 package](TEST_REPORT_PHASE30_EXPLORER_TREE.md) remains
the latest fully verified distribution until this checkpoint finishes validation.

## Required contract

At most 64 Back and 64 Forward locations plus one pending admission ticket are
retained. Locations bind the connection generation, exact document identity and
edit version, plus complete selection endpoints/direction/affinity. They retain
no source text, URI/path or duplicate Undo history. Saturated versions fail closed.

Only successful explicit navigation admits a departure once. Chooser opening or
cancellation, ordinary cursor movement, edits/Undo, saves/reloads, close fallback
and stale background Reads do not admit history. Existing asynchronous path,
range, session, generation and newer-intent guards remain in force through actual
selection completion. Back/Forward issues no worker operation.

Before applying an actually ready, currently owned navigation reply, substantive
input in that same frame takes precedence over its foreground/history effect.
Ordinary file Reads may still create a background buffer. Earlier caret movement
while no reply is ready preserves the captured departure. Typed language-read
origin and Definition dispatch ownership remain bound across cancellation; reply,
save acknowledgement and terminal-event order is unchanged.

A destination at the edit-version ceiling may still receive ordinary navigation,
but history is visibly unavailable and both stacks stay unchanged. A saturated
departure is omitted when navigating to a valid destination; Forward still clears.

Edited or closed locations are explicitly skipped/pruned rather than clamped.
Pruning cannot mutate the opposite stack; only a successful target restore pushes
the current location there. All-stale attempts preserve the current document and
selection. Reconnect clears history; no closed file or discarded draft is restored.
Traversal also removes valid entries exactly equal to the current full selection
so they cannot hide an older distinct target. These are redundant entries, not
stale ones. An all-equal traversal preserves the current location and opposite
stack without pushing history; same-location new navigation remains a no-op.

## Finite acceptance

Required deterministic cases cover every admission route, partial/all-stale stacks,
edit then Undo, affinity and reversed Unicode selections, close/reopen identity,
saturation, asynchronous stale/newer-intent cases, keyboard/modal ownership and
complete draft/baseline/Undo preservation. Actual normal-agent tests must execute
on both OS, separating initial open Reads from zero-operation history traversal,
with unchanged generated source hashes and bounded cleanup. Cloud Linux native
trust-off checks cover controls/chords, visible selection, typing and Undo/Redo;
they do not establish native Windows GUI or SSH behavior.

## Local verification

An initial cloud-native check found that the first history-button click after
editor focus was swallowed. A production-frame regression reproduced it before
the repair: IME lifecycle housekeeping cleared the captured button press. The
button classifier now ignores only lifecycle and empty composition events;
nonempty composition and paste still cancel competing clicks. The regression
matrix covers both directions and queued replies. The original failed check is
retained; the repaired default release passed a fresh native check below.

The final host aggregate passed 1,230 tests with zero failures and 31 explicit
opt-ins ignored across 41 suites. The app portion passed 745 tests. Strict
all-target/all-feature Clippy passed for the host and MSVC target; all-feature
and default release builds passed. MSVC checking is compilation evidence, not
native Windows execution.

The actual Linux normal-agent history test passed: 10 cases, 20 production
shortcut attempts, two connection Lists, four explicit Reads (three initial and
one reopen), zero Writes/other operations and zero traversal operations. It
verified all three generated source hashes, full selection and dirty Undo state,
two reaped agents and fixture removal. The same test is required on both CI OS.

The package/link suite ran 36 tests: 35 passed and one platform-specific case was
skipped. Exact-commit dual-platform CI/package acceptance remains pending. No
capability, backend permission, dependency or profiling change is part of this slice.

## Cloud Linux native check

The repaired default release passed trust-off checks with three generated Unicode
files. First Back and Forward pointer clicks from editor focus each moved once
and restored full reversed selections. Ctrl brackets and visibly focused Tab/Enter
buttons worked; typing after return, Undo/Redo, edit-then-Undo stale skipping,
plain bracket input, excluded modifier combinations and chooser cancellation passed.
No Save was used. All three source hashes and both binary hashes were unchanged
after owned application exit with code zero.

A subsequent publication review identified a current-equal traversal barrier.
Both directional reproductions failed before the narrow pruning repair; seven
new direct/actual-frame regressions now cover it, including full cursor affinity.
The final release received a targeted native pointer/selection recheck. Visible
same-buffer caret returns could require one restoration at the same displayed
line before continuing in either direction; the native UI does not expose hidden
affinity, so these observations are not an exact-equality proof. Exact equality
and all-equal pruning are asserted by the deterministic frame tests.

History controls and editor actions remained reachable at 1178×814 and 780×540
with minimum/default/maximum sidebar widths. At 780×540 with a 460-pixel sidebar,
the third tab label and long editor text extend beyond the center viewport; this
does not establish a general responsive-tab or forms fix. The middle width at
minimum size was manually approximated. Real IME composition, native Windows GUI
and real SSH were not exercised. Screenshots remain outside the public source.
