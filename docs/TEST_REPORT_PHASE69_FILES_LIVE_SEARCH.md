# 0.69 Files search-as-you-type — candidate verification

## Status and accepted baseline

**0.69.0 is a candidate and is not yet accepted.** The approved slice is Files-only
search settlement after a fixed 250 ms pause. Final versioned local aggregate and
strict checks, independent input review and four current-artifact process proofs
passed. Final sealing, exact dual-platform CI, three package audits and any native
journey remain separate pending gates. Local passes do not establish those results.

The latest accepted baseline is **0.68.0 current-file Find**, whose
[public source revision](https://github.com/LLLLimbo/cedar-ide/commit/a525c547aeb2f1fc1d80d3d872801c1f4d986963)
passed all four jobs in
[CI 38444382093](https://github.com/LLLLimbo/cedar-ide/actions/runs/38444382093)
and all three exact-source package audits. Linux recorded **2,176/0/71**, Windows
**2,114/0/114** (passed/failed/ignored), each across **50 suites**. Browse v5 passed
57 fields and 14 operations, including zero added current-file Find operations.

The second bounded Trust-off cloud Linux native journey passed core selection
seeding, F3/Shift+F3, visible Next/Previous, Escape/Back, no-hit stability and dirty
Undo/Redo, with timing and integrity independently checked. The first attempt
remains incomplete after its startup observation exceeded the budget before
Connect or functional input; no product startup-failure inference is made.
Native cap behavior, raw selection affinity, request counts, large buffers, tiny
viewports and broad IME/CJK remain unmeasured. Positive File Structure/JDT GUI,
native Ctrl+F12, Windows/macOS GUI and real SSH limitations also remain. See the
[accepted follow-up](TEST_REPORT_PHASE68_CURRENT_FILE_FIND.md#accepted-0680-follow-up)
and [current report](TEST_REPORT.md#accepted-0680-bounded-baseline).

## Candidate interaction contract

The [offline guide](FIND_AND_RETURN.md#files-search-as-you-type-069-candidate)
contains the complete usage instructions and visible recovery messages.

- Only **Files** query and **Directory scope** edits settle automatically. Each
  valid committed edit starts a fresh fixed **250 ms** interval for the complete
  query/directory pair. Repaint, caret movement and no-op Delete do not extend
  that interval. Fresh committed Text/Paste or a clear terminal Commit gets a
  new identity even when the final bytes equal the prior value. Invalid input
  cannot cause a speculative backend request.
- **Enter / Find** flushes the current candidate immediately and cancels its
  timer. An identical in-flight or latest pending request is deduplicated;
  ordinary deliberate reruns after completion remain explicit.
- Keep one FindFiles in flight and at most one latest replacement. Editing
  immediately retires old rows, pointer and activation tickets, and preview.
  A → B → A requires a fresh identity; late success or error for the first A
  cannot restore it. This retires frontend intent, not the backend transport or
  a blocked filesystem call.
- Automatic settlement and accepted/late replies never transfer keyboard focus.
  Hide, Close, Escape, provider/context/connection change and competing modal
  ownership retire pending work. Reopening does not replay cancelled intent.
- An empty root query uses only open/loaded context, with zero FindFiles. An
  empty query plus nonempty scope is invalid and cannot broaden the search.
  Old peers and unsupported extension versions retain labelled local snapshot
  filtering, with zero FindFiles and no discovery directory scope.
- Busy, invalid, unsupported and error outcomes do not start automatic polling,
  retries or idle resubmission. A new valid committed edit or clean explicit
  submission is required to try again. The one latest replacement behind a
  running Files request is not a retry loop.
- **Project Text remains explicit.** It keeps its current query, filter,
  retained-result and Preview behavior. Files automatic search admits no
  content Read, Open, Save, document edit, Undo reset or location-history move.
  Files Preview stays explicit; existing dirty-buffer reuse remains zero Read.

### IME boundary

The query and scope are owned by their existing TextEdit controls. Active
composition cannot admit an automatic request. A clear terminal nonempty Commit
may start the entire 250 ms interval, including when the final bytes match a
previous query. Ordinary query/scope validation remains required.

An empty Commit, Disabled without a clear terminal commit, or a new preedit
followed by an ambiguous ending/cancellation retires settlement and displays
**Automatic search paused after composition. Edit the query or press Find to
search.** The implementation must preserve visible query/scope bytes exactly;
no rollback, saved-string restoration or input rewrite is permitted. Resume
only after a fresh valid committed edit or clean explicit Find. Lifecycle
notifications and elapsed time alone do not resume a paused query. Supplied
input tests must distinguish event order; they do not establish broad native
IME availability or behavior.

### Unchanged discovery and product limits

Keep the existing negotiated `file_discovery` v1 contract, protocol and trust
meaning. Queries still use literal Rust Unicode lowercase matching, ranked by
exact basename, basename prefix, basename substring and path substring, then
original path. No CamelCase, regular expressions, normalization or full case
folding is added. The interface still returns at most 100 paths; traversal,
depth, cooperative time, pending-directory, path and response bounds remain.
Exclusions, symlink/reparse handling and incomplete reasons are unchanged.
No match from an incomplete scan is not proof that a file does not exist.

The fixed pause is a request-admission rule, not measured latency or a performance
claim. No index, watcher, hard cancellation, new provider, package-inventory
expansion, dependency or protocol extension is part of this slice.

## Browse v6 process contract and final verification gate

The receipt has **64 strict typed fields / 16 operations**. All 57 prior fields
remain. Restoring the old schema version and removing only the two new ledger
entries must reconstruct the exact version-5 contract, including every nested
value and the fourteen-operation prefix. The earlier version-4 contract remains
independently preserved. Existing current-file Find still adds zero operations.

The only operation suffix is:

1. `FindFiles("Needle", "untouched", 100)`
2. `FindFiles("origin.txt", "", 100)`

Actual production frames must observe burst query/scope settlement, latest-only
admission, no early request and immediate old-row retirement; explicit Enter/Find
must not duplicate the same pending request. Then explicit preview and ordinary
Open reuse the existing dirty origin with zero Reads, followed by exact Back.
The draft, revision, Undo and retained Project Text state must remain intact.
No automatic Read or extra FindFiles is permitted.

| Added strict field | Required value |
|---|---|
| `files_live_debounce_ms` | `250` |
| `files_live_burst_latest_only` | `true` |
| `files_live_explicit_flush_deduplicated` | `true` |
| `files_live_automatic_reads` | `0` |
| `files_live_dirty_preview_open_reads` | `0` |
| `files_live_open_back_exact` | `true` |
| `files_live_state_preserved` | `true` |

Missing, extra, false, wrongly typed or historical-schema observations and
extra operations must fail strict validation. An independent projection checks
the complete previous contract; checking only field counts would be insufficient.
The suffix has a **60-second** admission budget inside the unchanged
**240/30/300-second active/cleanup/watchdog** bounds. The existing driver's
compile/backend/app bounds stay unchanged. Trust off, source and fixture
integrity, one owned agent, verified shutdown and reaping remain required.

This process contract is headless. It does not establish native
250 ms wall-clock behavior, actual OS input delivery, broad IME behavior,
Windows/macOS GUI, real SSH or positive provider/JDT Structure GUI.

## Preserved development checks and corrections

The third local retained Browse run passed **83 tests / zero failures / one
intentionally ignored normal-agent test**, including **24 new Files checks**.
The compiler identified this development artifact as **0.68.0**. It is evidence
for the new source behavior before the final 0.69 version update, not a final
versioned aggregate, normal-agent process proof, CI, package or native result.

Ordinary timing, field edits and mixed-input checks use the production raw-input
hook and full update frames. Nonfinite or regressing clocks are supplied at the
production end-frame settlement seam after real UI input because egui rejects
backwards raw time before update. That clock check is not a full raw-event
reproduction. The tests also exercise no-op Delete without deadline renewal,
fresh same-final-value Text/Paste/Commit identity, and ambiguous IME endings that
preserve native query/scope text while pausing automatic admission.

The first retained run failed **four** tests (**73 passed / four failed / one
ignored**): two older discovery fixtures still assumed explicit-only timing,
the backwards-clock fixture stopped inside egui, and the held-pointer fixture
expected appended text without establishing the query's actual focus. The
second run still failed that pointer fixture (**77 passed / one failed / one
ignored**), observing `needle` rather than the expected `needlex`. Those runs
remain failed. The revised fixtures use bounded pre-settlement frames, the
identified clock seam, and actual Tab focus before text input; the stale-release
assertion remains. The later passing result is reported separately and does not
reclassify the original failures or imply that a native input issue was fixed.

### Early normal-agent proof failure and bounded correction

The first early Browse v6 normal-agent execution failed its new target-preview
assertion: the observer returned `None` instead of the expected file text. The
file had already been loaded normally, so preview correctly used the existing
`Draft` representation. That assertion called a disk-only observer. The second
new dirty-preview assertion used the same observer but was not reached.
The failed run remains failed, with its original receipt and unchanged fixture
retained; verified close still completed. It is not evidence of a production
preview failure.

The correction changes only the two new proof observer calls to the existing
current-ticket-aware helper, which handles both Draft and Disk preview. Six
positive assertions additionally check the selected row, actual row focus and
visible preview at both points. No production code, input sequence or admission
guard changed, and no original proof assertion was removed.

The second early run passed its single selected test and strict **version-6,
64-field / 16-operation** receipt. It retained the original fourteen operations
and added exactly the two specified FindFiles calls. Automatic Reads and Reads
for the new dirty preview/Open were both **zero**; all new positive observations
passed. Trust stayed off, source/fixture and selected-artifact bindings were
verified, one owned normal agent was explicitly reaped, and the successful
fixture followed the existing verified-close cleanup path.

These were default-feature development artifacts still reporting **0.68.0**.
The canonical compile driver was not run for this early diagnostic. The pass
remains separate from the later versioned local 0.69 results below; it does not
establish exact CI, package or native acceptance.

## Final versioned local checks

The **0.69.0** workspace/all-target/all-feature locked Rust aggregate passed
**2,200 tests / zero failures / 71 ignored across 50 suites**. The application
suite recorded **1,484/0/54** and included all 24 new Files checks. Strict
workspace/all-target/all-feature Clippy with warnings denied and formatting
passed. All **29 Python suites** passed: **805 tests total, ten skipped, zero
failed suites**. Source inputs remained unchanged during the Rust aggregate.

The first versioned aggregate remains failed: the application suite recorded
**1,483 passed / one failed / 54 ignored** and stopped on the retained welcome
entry test's old **Project files · explicit filename search** label expectation.
The approved correction changed only that expected prefix to **Project files ·
search after a short typing pause**, preserving the visible welcome-entry and
zero-request assertions. This intentionally updates the UI-copy contract for
Files live search; it does not change production logic or discard an assertion.
The second complete aggregate passed separately, as recorded above.

Independent input review and its proof-observer and final-version addenda passed
with no unresolved findings. The final addendum checked exact version-only
synchronization, ten Cedar lockfile version changes with external dependencies
unchanged, and the single expected-label correction. It retains the no-rollback
IME decision and the distinction between actual input frames and the clock seam.

The Cargo-JSON-selected current default-feature application and matching normal
agent passed all four strict local process proofs:

| Proof | Strict fields | Operations |
|---|---:|---:|
| File Structure | 45 | 11 |
| Browse v6 | 64 | 16 |
| Scoped Search | 57 | 10 |
| Search | 57 | 12 |

Browse v6 retained every prior field/prefix requirement, exactly the two new
FindFiles operations, zero automatic Reads, zero Reads for the new dirty
preview/Open, Trust off and one verified owned-agent reap. The File Structure
proof uses the explicit test-server mock; it is not a native real-JDT journey.
These are current selected-artifact normal-agent executions, with source and
artifact bindings verified. The canonical compile drivers were not run locally.
None of these passes establishes the remaining exact CI, package or native gates.

## Validation gates

| Gate | Current state and limit |
|---|---|
| Files-focused and retained Browse checks | All **24 new Files checks passed** within the final versioned aggregate. The earlier development 83/0/1 result, clock-seam boundary and failed attempts remain separately recorded above. |
| Independent source/input review | **Passed**, including proof-observer and final-version addenda; no unresolved findings. Original input ownership and no visible TextEdit rewrite remain required, including ambiguous endings and clear terminal commits. |
| Strict Browse v6 parser checks | **16/16 passed** for the authored v6 contract. The static audit confirms 64 fields, 16 operations, the exact two-query suffix and exact v5 projection, with driver bounds unchanged. This is parser/static evidence only; no Rust process or application runtime was executed by these checks. |
| Workspace aggregate, strict Clippy, formatting and Python | **0.69.0 passed:** Rust **2,200/0/71 across 50 suites**, strict Clippy and formatting; Python **805 total / ten skipped / zero failed suites across 29 suites**. The first versioned aggregate failure is preserved above. |
| Current-artifact Browse v6 and retained process proofs | **0.69.0 passed:** Structure **45/11**, Browse v6 **64/16**, Scoped Search **57/10**, Search **57/12** (fields/operations). Browse adds exactly two FindFiles and zero automatic Reads. These are selected-artifact local proofs; Structure uses a mock and canonical compile drivers were not run locally. |
| Offline guide/package closure | **Passed:** the three existing current-guide closure tests for Windows, Linux agent and Linux desktop. All **184 local links across eight edited documentation files** resolve, including anchors. The original phase 68 preparation content is preserved exactly beneath its accepted follow-up. These are documentation checks, not new package or runtime acceptance; bundle inventories remain unchanged. |
| Exact dual-platform CI and all three source-linked packages | Pending. |
| Bounded native Files journey | Pending. Any later claim must name the actual artifact, platform, journey, timing and integrity checks; broad native IME remains outside this contract. |

All newly failed or incomplete attempts must retain their original result and
scope. A corrected test or later pass cannot turn an earlier failure into a pass.
The accepted 0.68 native startup observation limit, earlier 0.67 shortcut entry
failure, 0.66 preview/test failures, 0.65 glyph stop and 0.64 strict timing failure
remain preserved in the [current report](TEST_REPORT.md).
