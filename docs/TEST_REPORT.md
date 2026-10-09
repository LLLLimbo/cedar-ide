# Verification report · Bounded lazy Explorer tree / 0.30.0

This checkpoint adds an optional lazy tree alongside the default flat Explorer,
using the unchanged List/Read protocol and trust-off file path. There is no new
capability, background scan, watcher, automatic retry or execution permission.

## Required boundaries

One browser List may be in flight across modes. Requests bind connection, branch,
epoch, path and wire ID; invalidation does not free an outstanding transport slot
until its matching completion. Immediate-child metadata is checked before cache
admission. Refresh replaces old data only after the whole reply fits the bounded
cache; errors retain clearly stale data. Collapse releases descendants and moves
affected selection to the ancestor without changing documents.

Keyboard ownership, selected-directory scope, unknown versus empty directories,
Unicode byte accounting and visible limit refusal are required. Existing dirty
Open, selection, Undo/recovery and sidebar/tool-access behavior remain required.
Save acknowledgement marks the saved parent stale in tree mode without issuing
an implicit directory request.

## Finite acceptance

Headless tests cover bounds, stale success/error, mode/reconnect, transactional
refresh, scope, keyboard/modal ownership, dirty state and retained sidebar cases.
The normal-agent test must run on both OS with generated Unicode fixtures and
execution trust off, recording only explicitly admitted List/Read calls, no Write
or tool operation, unchanged source hashes and joined cleanup. Cloud Linux native
CUA checks pointer/keyboard nested browsing, final-row access and draft Undo/Redo
at small and normal sizes; this is separate from Windows GUI and SSH evidence.

## Local verification

- Independent state/protocol and UI/input reviews found no remaining blocker.
- Strict host and Windows MSVC cross-target Clippy passed, as did formatting.
- The full local Rust run passed 1,163 tests with 30 platform/opt-in tests ignored.
  All 28 tree tests, 14 retained sidebar tests, 17 workspace-access tests and four
  tooltip tests executed successfully. Ignored cases are not counted as passes.
- All-feature and default-feature release builds passed.
- The actual normal shipping-agent test executed 11 cases with 13 Lists, two Reads,
  zero Writes and zero other operations. It verified 4,102 generated source hashes,
  two reaped agents, the joined watchdog and fixture removal. Trust stayed off.
- Package/link tests ran 36 cases: 35 passed and one platform-only case skipped;
  all five capability-smoke tests passed. The Explorer guide is explicitly packaged.

Review and retained regressions caught input-repeat and nested-scroll issues before
sealing. Tree-owned held keys are suppressed through release, including file-open
focus transfer; explicit same-row navigation reveals its target without idle scroll
snapping. Retry IDs bind their actual paths, and changing sibling/status counts
cannot move a captured click to another target. Explorer background-content dragging
is disabled to avoid stale egui hit regions; wheel, touchpad and scrollbars remain.

## Cloud Linux native GUI

The exact default-feature release binary was exercised through the cloud Linux
windowing environment with execution trust visibly off and a generated 55-file
workspace. Checks passed at 780×540 with the minimum sidebar width and at 1178×814
with narrow/default/wide sidebars:

- Flat default, explicit unloaded Tree root and chooser scope, nested Unicode
  expansion, retained siblings, collapse/reopen/Refresh and loaded-empty labeling;
- final wrapped Unicode file, wheel and scrollbar-thumb navigation, and explicit
  same-row End revealing the selected row after scrolling away;
- reopening an existing dirty buffer while holding Enter for 1.5 seconds without
  newline leakage, followed by exact Undo/Redo and full-selection retention;
- Tab reachability of Flat/Tree and all five existing tool selectors without
  activating execution tools.

The owned app exited zero, all 55 source paths and file hashes remained unchanged,
and the owned terminal closed. No Save, execution-trust activation or SSH operation
was used. This is Linux GUI evidence, not native Windows GUI validation. Existing
full-height tools/center responsive limitations remain separate.

Exact published dual-platform CI and Windows package validation are pending.
[0.29.1](TEST_REPORT_PHASE291_CAPABILITY_INVENTORY.md) remains the latest
fully verified distribution. Normal Windows has 31 capabilities under the unchanged
32-capability limit; this feature consumes no additional slot.
