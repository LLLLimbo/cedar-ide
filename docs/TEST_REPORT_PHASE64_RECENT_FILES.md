# 0.64.0 Recent Files bounded functional acceptance

**0.64.0 is the latest accepted bounded functional baseline. The strict native
timing gate failed; this is not an all-gates pass.**

## Accepted source, CI and bounded native result

Public commit
[`cf6fbb4c3b7b3149a1c345a1485ccbb9b50ff49e`](https://github.com/LLLLimbo/cedar-ide/commit/cf6fbb4c3b7b3149a1c345a1485ccbb9b50ff49e)
passed all **four jobs** in
[CI 38354618490](https://github.com/LLLLimbo/cedar-ide/actions/runs/38354618490)
and all **three exact-source package audits**. Each platform ran **50 suites**:
Linux **2,082 passed / zero failed / 70 ignored** and Windows
**2,020 passed / zero failed / 114 ignored**, including the **19 new Recent
Files/control cases**.

The bounded Linux native run passed its essential functional and integrity
checks. Quit initiation was **20.436 seconds after the required deadline**,
so the strict timing gate failed. That failure is retained alongside the
functional result; acceptance of the bounded functional baseline does not turn
it into a timing pass or establish an all-gates result. No later native rerun is
claimed here. Windows/macOS GUI, real SSH and arbitrary viewport behavior remain
outside this acceptance.

The preparation results below predate that checkpoint. Statements that exact CI,
package and native checks were still pending describe their historical stage;
they do not make the current functional baseline pending again. New 0.65 work
requires independent validation.

## Recent Files contract

Recent Files retains at most **64 current-connection paths**, **256 KiB of UTF-8
path bytes in total**, and **4,096 UTF-8 bytes per path**. Only a settled active
file with a known revision may enter the list, including a previously unsaved
active tab after its first successful Save. Closing a file retains its path,
not its text, caret or selection. Clear, disconnect, reconnect, workspace change
and restart empty the list. Idle repaint cannot repopulate it.

The shortcut is **Ctrl+E on Windows/Linux** and **Cmd+E on macOS**; macOS Ctrl+E
keeps its editor behavior. Filter recent paths uses case-insensitive substring
matching, at most 4,096 Unicode characters; path deduplication remains exact.
Rows carry Open/Closed badges. Clicking selects; Enter or Open/Open closed file
performs explicit opening. Clear recent clears the list and closes the chooser;
Close/Escape dismisses it. This connection only describes its scope.
Filtering is local and does not dispatch an operation. Opening a live dirty
buffer reuses it with zero Read; a closed path follows ordinary Open/Read with
the ordinary Open default caret position. No closed caret or selection is stored
or restored, and no line-1 position is promised.
Current session, document, input and activation guards must still apply.

No preview, edited-only toggle, index, new protocol/backend operation, durable
history, saved caret or parity claim is part of this slice. The
[usage guide](RECENT_FILES.md) keeps Recent Files distinct from filename discovery,
search results and precise open-buffer location history.

## Final-version local checks

The final **0.64.0** Linux workspace/all-target/all-feature/locked aggregate
recorded **2,082 passed / zero failed / 70 ignored across 50 suites**, including
app **1,366/0/53** and **19 new Recent Files/control tests**. The updated Python
aggregate reported **791 total tests / zero failures / 10 skipped across
28 suites**. Strict Clippy and the final formatting check passed.

The first strict Clippy attempt rejected a test fixture's field reassignment
after default construction. The fixture now uses the equivalent direct
initializer, and the strict rerun passed. This changed no production semantics
and used no lint waiver; the initial failure remains part of the local record.

### Prepublication package-documentation correction

A prepublication check found a gap in bundled documentation: the Recent Files
guide needed to be included alongside the guides linking to it. All three
bundle builders now include it. Package validation checks nested relative guide
links within explicit bounds, and report links use the public repository URLs.
The focused package suite reported **119 total tests / zero failures / one
skipped**; this does not establish the still-pending exact-source package audits.

Three new corruption tests initially failed because their mutations changed
file length and reached the size check before the intended SHA check. Same-length
fixture mutations now exercise that hash boundary. The initial failures remain
recorded; validation was not weakened. Application Rust, Cargo inputs and workflow
bytes were unchanged by this packaging correction, so the Rust results above
are retained; no additional Rust run is claimed.

## Preserved preparation and caret-assumption failure

An earlier focused checkpoint passed **18/18 tests**, and formatting passed.
This is preparation evidence, not the final integrated or acceptance result.

The initial normal-agent process attempt failed a caret assertion: it observed
**line 4, column 1** where the test assumed **line 1, column 1**. The failure is
preserved. Inspection and actual-frame controls established that the pinned egui
editor's ordinary fresh Open defaults to the end of the file; Recent Files was
not restoring a remembered caret. The production ordinary Open behavior was
left unchanged.

An actual-frame control passed by comparing Recent reopening with settled
ordinary Open of the same bytes, including the exact cursor and selection.
It also passed with changed, shorter file bytes, a new document and the current
revision, without restoring a historical caret. The process proof now binds its
expected position to the exact ordinary-Open baseline. A subsequent preparation
checkpoint passed **19 focused tests** and the complete **eight-operation Browse
runtime proof** before the final version bump. These are successful earlier
results, not passes for the subsequently selected 0.64.0 artifacts.

## Preserved process proof and then-pending validation

The final 0.64.0 normal-agent Browse proof retains the existing
**seven-operation prefix and all its assertions**, then appends a Recent Files
closed-file reopen and dirty-buffer reuse. Its new contract is **eight total
operations / 29 fields**, using a version-2 receipt and a distinct test entry
point. Existing historical seven-operation receipts keep their original meaning. The initial
failed attempt, subsequent caret control and passed pre-version-bump runtime
receipt remain separate. The final rerun used the current 0.64.0 app selected
from Cargo JSON and the matching current agent. Both the **eight-operation
Browse proof** and retained **ten-operation scoped-search proof** passed their
strict validators.

These were direct local process proofs. The canonical compile/release pipeline
was not run locally; its existing driver/workflow budgets remain unchanged.
Exact-commit dual-platform CI, all three exact-source package audits and a fresh
bounded native journey remain pending. Local results and earlier 0.63.3
acceptance cannot substitute for those gates.

## Preserved history

The 0.63.0 menu observation, 0.63.1 Find Results observation and input-route
limitations, 0.63.2 ambiguous Flat-mode focus witness, earlier unavailable
original evidence, and the local diagnostic's different setup and timing limits
remain in the [verification history](TEST_REPORT.md). Accepted 0.63.3 evidence does
not reconstruct missing records or identify earlier native event batches.
Windows/macOS GUI, real SSH, arbitrary viewport behavior, IDEA parity and
comparative performance remain outside the established acceptance.
