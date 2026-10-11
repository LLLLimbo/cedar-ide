# Verification checkpoint — 0.53.3 selected-slice replacement

## Scope and acceptance state

0.52.2 is the latest accepted checkpoint; its exact CI, native scope and preserved failures are recorded in [the coordinate report](TEST_REPORT_PHASE52_COORDINATES.md). 0.53.0 adds a separate **Preview selection** action to literal current-buffer replacement. Implementation is pending verification. Existing whole-file Find, Preview one and Preview all remain available. No new remote capability, dependency, regular-expression engine or implicit Save is added.

The new action requires an exact nonempty editor selection and searches that selected slice independently using case-sensitive, nonoverlapping literal matches. It never expands the selection or normalizes newlines. Every byte outside it is preserved. Source, replacement and result each remain within 1 MiB; at most 10,000 matches inside the selected slice are admitted. Empty, missing, stale, invalid or excessive inputs refuse without a text transaction.

Preview captures document/path/version, workspace/session/navigation identity, query/replacement and both cursor endpoints including affinity. The new Selection buttons additionally use original-frame input ownership so an Apply click cannot hide competing text or paste when a field loses focus; mixed and stale gestures are refused without replaying input. Final-frame Apply revalidates its captured state. Effective Apply is one draft transaction with exact selection Undo; equal-text results leave version and Undo/Redo untouched. Saving remains separate. Already enabled language synchronization follows ordinary draft edits; zero-operation proofs are specifically Trust-off without an active language service.

## Required normal-agent proof

Sixteen fixed cases cover forward/reversed LF/CRLF selected slices, Preview/Cancel, complete Undo/Redo and no-op history, empty selection, crossing matches, stale selection/affinity/edit/workspace/session/tab, explicit reconnect, and a fresh replacement afterward. Setup produces exactly Hello, root List and two explicit Reads; reconnect adds one Hello and one root List. The fixed 41-field receipt requires six total operations, two reaped owners, zero operations during replacement actions, and both original disk hashes.

One shared active budget is 60 seconds with a five-second cleanup reserve inside 65 seconds total. Each Client request must admit its full 30-second bound. Uncertain cleanup retains the generated root and fails without a success receipt. The independent driver bounds compile/selection/runtime to 120/15/90 seconds with bounded subprocess teardown inside the five-minute CI step. These are test budgets, not changes to shipping deadlines.

## Required evidence

Independent bounds/input/history and process-ownership reviews, pure and actual-frame tests, both-platform normal-agent ledgers with unchanged fixture hashes, and a fresh exact-package cloud GUI check are required. The GUI gate covers selection preview/cancel/apply, outside-range preservation, Undo/Redo and explicit discard, with no Save, Run or LSP. Process ledgers provide request counts; GUI observations do not independently measure network traffic.

The authored replacement suite contains 31 tests, including 16 additional groups, with existing one/all assertions retained and stale-state matrices expanded. Tests cover selected-slice overlap alignment, dense outside matches, literal Unicode/CRLF bytes, full-selection affinity/history, no-op redo, bounds, minimum-window controls, actual pointer/keyboard activation, mixed and split gestures, stale or missing input capture, and sizing retries. These Rust tests have not been compiled or executed locally.

Local Rust compilation, aggregates, releases and native process execution are deferred to fresh CI because disk reserve is constrained. The new strict Python receipt driver passes 12 tests; its shared bounded supervisor passes 21 and the retained coordinate driver passes nine. Package tests pass 39 Linux desktop, 30 Linux agent and 36 Windows cases (one platform skip); public-export tests pass two. Structured lockfile comparison confirms only ten Cedar package versions changed, with third-party versions and dependency features unchanged. Formatting and independent source review are checked at seal. Fresh dual-platform strict compilation, aggregates, process runtime, all packages and exact GUI remain pending. Production Windows recovery policy is unchanged; the nonshipping admission investigation stays paused. Historical Git incomplete-EOF and client marker failures remain without an established root cause.

## Preserved 0.53.0 first-compilation failure

Public `3ba65799f9896f94b88ae7d087200a12dd8a568b`, [CI 38113209812](https://github.com/LLLLimbo/cedar-ide/actions/runs/38113209812), failed the first strict Rust compilation on both OSes: the single 41-field test receipt `serde_json::json!` exceeded the default macro recursion limit. No Rust test, process proof, release build or package ran; no GUI artifact existed. This failure is preserved.

0.53.1 only splits that test receipt into a 20-field metadata object and 21 explicit true boolean fields. Duplicate insertions and an unexpected final count fail before printing success. The same exact 41 fields/types/values and strict Python validator remain required. No recursion-limit increase, lint suppression, shipping behavior, runtime budget or dependency change is introduced. Local Rust compilation remains deferred; fresh full CI and exact-package GUI are mandatory.

## Preserved 0.53.1 strict-check failure

Public `5f1236bf239db972a322ec942633006b8f12aa1f`, [CI 38113714668](https://github.com/LLLLimbo/cedar-ide/actions/runs/38113714668), expanded the corrected receipt macro but failed strict Clippy on both OSes: two test helpers passed an unnecessary extra reference to `admitted`. No Rust tests, process receipts, release builds or packages ran.

0.53.2 removes only those two extra borrows where `app` is already a reference. The other calls borrow owned fixture values and remain unchanged. No lint allowance, assertion, shipping behavior, receipt schema or runtime budget changes. Fresh dual-platform strict checks, all tests/gates/packages and exact GUI remain required; both prior failed runs remain failed.

## Preserved 0.53.2 layout-fixture failure

Public `a4f866580d8b7b751dec9e2fbfe94a2ad855ed74`, [CI 38114122961](https://github.com/LLLLimbo/cedar-ide/actions/runs/38114122961), passed strict compilation on both OSes, then failed the held-click layout test's off-point precondition. Both platforms passed 30 of 31 replacement tests; no process proof or package was produced. The raw logs do not identify the Preview/Apply iteration or record its rectangles.

0.53.3 changes only that test's geometry observation to the existing recorder of the rendered `Response.rect`. Pinned egui 0.31.1 swaps pass state at the end of a pass, while `read_response` prefers its current-pass storage; a post-frame reconstructed response can therefore describe prior interaction geometry. The test now verifies rendered movement, stable button ID and the same captured owner, while retaining the unchanged click point, one intervening idle frame, real press/release, off-point precondition and successful action assertions. Contextual rectangle diagnostics identify either button if fresh CI fails. No additional settling frame, sleep, shipping behavior, receipt schema or budget changes. Local Rust execution remains deferred; fresh full CI and native acceptance remain required.
