# Verification report · Keyboard navigation / 0.18.0 · 2026-10-08

This checkpoint adds bounded frontend navigation: a keyboard-selectable chooser
for existing buffers and files in the already-loaded current directory, plus
Go To Line. It introduces no backend operation, recursive index, background scan
or project persistence. Both features are available with execution trust off.

The previous [0.17.0 exact Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37811229491)
passed, including real asynchronous Java startup and all previous semantic,
cleanup, Git and bundle checks. Its completed evidence is preserved in the
[phase-17 report](TEST_REPORT_PHASE17.md). The earlier intermittent JDT diagnostic
publication cause remains unresolved; explicit refresh remains a mitigation.

## Navigation contract

Ctrl/Cmd+P combines open buffers and current-directory files, deduplicates exact
paths and shows at most 64 substring matches. Open buffers appear first. Up/Down
selects a result and Enter opens it. An explicit typed-path action remains
available. Selected paths retain their literal spelling, including Unicode.
The chooser does not enumerate other directories or traverse the workspace.

Opening reuses existing document ownership and asynchronous navigation checks:
dirty buffers are retained, pending reads are deduplicated, stale replies cannot
steal focus from newer navigation, and the 32-buffer limit remains enforced.

Ctrl/Cmd+G accepts a valid 1-based line in the current document. It moves the
cursor through the existing editor navigation path without changing text,
saved revision or edit version. Cursor navigation retains native Undo/Redo.

## Acceptance scope

Headless event tests must cover opening and repeated shortcuts, modal keyboard
focus, typing/arrow/Enter/Escape isolation, exact Unicode paths, dirty-buffer
reuse, tab limits, stale reads and navigation-only Undo/Redo. Go To Line must
reject invalid, overflowing and out-of-range input against the current document.

Native Linux verification, if available through the cloud desktop, uses only a
new synthetic workspace with execution trust off. Windows correctness remains
subject to exact native CI; a headless bundle probe does not establish Windows
GUI acceptance. Authenticated SSH and GUI Trust-on testing remain separate gaps.

## Verification status

The final local aggregate passed 793 Rust tests across 40 suites, with 22 opt-in
cases left to their explicit native/process CI stages. All 15 new navigation
regressions and the populated-completion precedence test passed. Strict host
and MSVC Clippy and formatting passed. The normal default-feature release
app/agent build passed, followed by actual-agent protocol, capability, language
bridge and task bridge smoke tests. Python checks passed: export two, bundle 28 plus one
platform skip, Git fixtures six, crash collector 70 plus two platform skips,
process observer 79 and GC collector 36.

The first aggregate attempt ran out of build-cache disk space before tests. A
reviewed removal of obsolete generated test executables restored space while
keeping source, logs and current/delivered artifacts. The next run exposed modal
sizing/focus bugs, which were corrected and covered by the final passing suite.
The initial disabled sizing pass now retains bounded keyboard input for exactly
one interactive processing; cancellation or identity changes discard it. This
is frontend input handling and does not replay backend operations.

## Native Linux trust-off navigation

The final default-feature release was exercised through the cloud Linux desktop
on five newly generated synthetic files, with execution trust kept off. The
chooser passed current-directory filtering, keyboard Up/Down/Enter, open-buffer
ordering and deduplication, explicit nested typed paths and literal Unicode
selection. Dirty-buffer reuse retained edits without duplicating a tab. Go To
Line passed valid, invalid and Unicode line targets. Typing immediately after
Enter or Escape worked without clicking the editor; Undo/Redo remained usable.
After changing the current directory, an unopened root-only file was absent
while existing buffers remained available. All five on-disk file hashes matched
the initial baseline; temporary edits were undone without saving.

Tested Linux frontend SHA-256:
`ddd46f343c222c1c7ebd05712ce9aa4288e2545b339ba06bb3b182781a642394`.
Tested adjacent normal agent SHA-256:
`c2c81bdf6f06a6367c279f4d53c3efb00a1e806cccb9d9faf81d2047364f2ff8`.

Exact native Windows execution and regenerated bundle verification remain
required for this commit. Linux window testing does not establish Windows GUI
or authenticated SSH acceptance.
No resource-reduction or IntelliJ IDEA equivalence claim follows from this slice.

## Completed native CI and bundle verification

Exact public commit `13bece6c98072ff6e9b2e79117c754dd348fc20a` passed [Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37818623076). Windows executed 25 process lifecycle and 17 transport tests; real Git passed 1,399 assertions. Real Java production passed the asynchronous startup, semantic/editor and refresh witnesses, with natural root exit 0 and client reaping. All 525 portable ZIP payloads, source/run mapping and outer artifact digest were verified. These completed results supersede the pre-publication pending statements above. No Windows GUI, authenticated SSH or upstream diagnostic-cause fix is implied.
