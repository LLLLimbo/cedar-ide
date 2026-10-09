# Verification report · keyboard workspace access / 0.27.0

This bounded presentation-only change supplies explicit keyboard routes between
editing, directory browsing and the selected tools view. Hiding tools reclaims
workspace space while preserving drafts and tool state. It does not redesign the
expanded forms or guarantee simultaneous visibility of every control.

## Intended actions and invariants

- Ctrl/Cmd+J toggles tools. Hiding returns focus to the current editor, or an
  available Explorer anchor when no document is open. Showing retains the selected
  tool and focuses its rendered header selector. The existing close button shares
  the hide/return behavior.
- Ctrl/Cmd+Shift+E hides tools and focuses Explorer Refresh without issuing a read.
  If Refresh is disabled or unavailable, it does not substitute another tool.
- Documents, selection, Undo/Redo, recovery, unknown-save state, tool fields,
  results, filters and scroll state are preserved. These actions issue no worker
  commands and do not stop tasks or language services.
- One-shot focus yields to modal ownership, newer intent, generation/document
  changes and conflicting input batches. Native repeat does not repeatedly toggle.
  Mixed input batches conservatively ignore the access shortcut while preserving
  the original widget's other input, including text preceding the shortcut.

## Finite acceptance

Production-frame tests cover all five tools, populated reports, expanded Language,
empty/no-document cases, Find/Replace, resizing, modal/repeat/mixed input and focus
precedence. The existing 14 sidebar regressions remain required. A cloud Linux
trust-off native pass at 780×540 must demonstrate the keyboard route back to visible
editing, reopening unchanged tools, and Explorer traversal while preserving dirty
selection and Undo. Source hashes and owned cleanup are checked; no Save or
execution is part of the native pass.

## Current status

Independent source review is clear. All 16 workspace-access production-frame
tests and all 14 unchanged sidebar regressions pass locally. The access cases
include a zero-visible-editor close, batched close clicks, exact native modifier
forms, window focus loss, stale generation/document state, populated report
scroll retention and held-key preservation of a newer Find-field focus.

An earlier retained-sidebar run failed three cases after adding two visible help
lines. Restoring the original four-line footer and placing the new guidance in
existing tooltips resolved those failures without changing the sidebar tests.
Final-code strict host and Windows MSVC-target Clippy pass. The host aggregate
completed 40 suites with 1,070 passed, zero failed and 29 opt-in tests ignored;
required process/native opt-ins remain part of the full CI workflow. All-feature
and default shipping release builds also pass.

## Cloud Linux native workflow

The final default-release binary ran against a generated Unicode workspace with
execution trust off. At an observed 780×540 window, the expanded Language pane
crowded out the editor. Ctrl+J hid it, revealed the selected dirty text and directed
the next typed character to that selection. Undo restored the prior selected
text; reopening retained Language and its visible disabled default fields, with
the header focused. Ctrl+Shift+E hid it and focused Explorer Refresh. Keyboard
Tab/Enter opened both the first and final Unicode file. Returning preserved the
dirty draft and selection; Undo reached the original clean text and Redo restored
the edits. Keyboard activation of the tools close button also returned usable
editor focus. The normal 1178×814 window size was restored afterward.

These are cloud Linux native observations. Populated editable tool fields and
report scroll retention are covered by headless production-frame tests; the
trust-off native Language fields remained disabled. Full expanded-form layout,
Windows native GUI and real SSH are not covered. All 32 generated workspace
files retained their exact names and SHA256 hashes after the pass. The owned app
exited after explicitly discarding its draft, leaving no recovery draft records;
unrelated desktop windows were preserved. No project Save or execution was used.
The preceding
[0.26.3 acceptance](TEST_REPORT_PHASE26_COMPACT.md) remains the latest verified
checkpoint. Exact new-source checks, dual-platform CI and package verification
remain required. No Windows GUI, authenticated SSH or full responsive-form claim
is added.

## Final exact-source acceptance

Public `c9376f672da8b844981bb296a1ee743d1be5f62c` passed [Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37933400404). Both platforms executed all 16 access, 14 sidebar and four tooltip tests successfully. Required Java idle correction matched spontaneously in 39,649 ms, with no recovery attempt; the Maven pair passed in 28,644 ms. The verified Windows development ZIP contains 533 payload files plus its manifest, is 5,037,081 bytes, and has SHA256 `7f267cd1f3969b54a9036fbe1323553a3cde376b5c8e8274a8f980c5d0a92753`. This is verified package evidence, not a user-delivery, Library, Windows GUI or SSH claim.
