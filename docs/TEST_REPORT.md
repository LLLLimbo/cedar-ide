# Verification checkpoint — 0.73.0 Project Text layout candidate

## Current 0.73.0 Project Text layout candidate

The [phase 73 report](TEST_REPORT_PHASE73_TEXT_LAYOUT.md) records the Text-only
presentation scope, fixed status/footer and bottom read-only preview, exact
matching-line/file wording, independent review corrections, and the earlier
0.72.0-versioned development candidate's bounded Linux native comparison.
That candidate passed 194 focused checks with one existing ignored process test;
21 native captures and a normal rc0/470.633-second Connected run passed complete
postflight. Those images do not certify the final 0.73.0 binaries or measure
pending/loading/150 ms timing. Final-version full local validation and shipping
artifacts require separately source-bound receipts; external CI, packages and
any final shipping smoke remain distinct. No anticipated gate result is asserted
by this source report. Historical results below keep their original attribution.

## Prior 0.72.0 Files layout candidate

The [phase 72 report](TEST_REPORT_PHASE72_FILES_LAYOUT.md) separates the accepted
bounded Linux Files visual baseline from final-version local gates and the still
separate exact-source CI/package gates. Files rows use the full width with a
filename/parent hierarchy and explicitly requested lower preview; the action
footer stays fixed. Project Text and Recent layout are unchanged. Native pending
preview, narrow/large-font native windows, Windows/macOS GUI, and complete Unicode
grapheme rendering remain unverified. Final-version local Rust passed **2,240/0/71 across 50 suites** with default
parallelism; Python ran **821 tests: 811 passed, ten skipped, zero failed suites**
across 30 scripts. Strict workspace Clippy, formatting, aggregate release build,
and the installed-font CJK opt-in passed. Unmodified canonical drivers passed
Browse v7 **74 fields/17 operations**, Scoped **57/10**, Search **57/12**,
Structure **45/11** (mock LSP), and additional Report triage **55/10**, using one
independently frozen default-feature normal agent with pre/post artifact hashes.
Exact-source dual-platform CI and all three package audits remain pending.
Historical results below retain their original checkpoint scope and outcome.

## Prior 0.71.1 candidate 2

The [candidate 2 report](TEST_REPORT_PHASE711_CANDIDATE2.md) records the sole
license-test canonical-path oracle correction. Fresh Python verification is
811 passed plus ten skipped across 30 suites. All Rust/proof inputs are unchanged;
prior local and successful candidate 1 Linux CI evidence is reused explicitly
across commits, not presented as a new execution. Candidate 2 CI, three-package
acceptance and new GUI verification remain pending.

## Prior 0.71.1 candidate 1

Exact-source 0.71.0 [CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/38507906311)
failed Windows export-byte tests and one Linux compact-editor layout test;
no packages were produced and the JDT-dependent job was skipped. The
[0.71.1 repair report](TEST_REPORT_PHASE711_CI_REPAIR.md) records the exact
failures, deterministic reproductions and narrow production changes. Final
local Rust passed **2,235/0/71 across 50 suites** with default test parallelism;
strict Clippy and formatting passed. Python ran **820 tests: 810 passed and ten
skipped**, with zero failed suites. A final-source real CJK opt-in and all four
freshly pinned current-artifact process proofs passed. Dual-platform CI/package
acceptance and new native GUI coverage remain pending. No retry of the failed
source is counted as acceptance.

## Prior 0.71.0 main-window candidate

The main-window structure has an actual native visual baseline, with explicit
limitations and final-source regression work described in the
[0.71 report](TEST_REPORT_PHASE71_ISLANDS_SHELL.md). This does not claim full
IDEA parity, search-dialog reconstruction, automatic menu focus return or a
released package. Final local Rust passed **2,222/0/71 (50 suites)**, with
strict Clippy and formatting. Python passed **818 tests / ten skipped / zero
failed suites**, and all four retained current-artifact process proofs passed.
Exact-source CI and package acceptance remain
separate gates. Historical sections below preserve their original outcomes.

## Prior 0.70.1 Project status wording candidate

This patch only makes a completed Project selection identify its historical target:
`Selection completed: /path`. A new Recent, tab or Back activation does not alter
the retained Project scope, list or selection and must not dispatch an automatic
List or Read. Only this generated success label is single-row truncated; the full
UTF-8 path remains available on hover. Other status layouts and all request,
input, focus, cancellation and navigation lifecycles remain unchanged.
See the [0.70.1 report](TEST_REPORT_PHASE701_PROJECT_STATUS.md) for the source-bound
negative control, focused/aggregate checks and retained process-proof contracts.
Final local 0.70.1 checks passed: Rust **2,206/0/71 across 50 suites**, Python
**806 total / ten skipped / zero failed suites across 29 suites**, formatting and
strict Clippy. Current-artifact Browse v7 **74/17**, Structure **45/11**, Scoped
**57/10** and Search **57/12** proofs passed (fields/operations). Exact-source
[CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/38494545055) and three
package audits subsequently passed; the native wording run was paused to
prioritize main-window visual work, without advancing native acceptance.
No new feature, operation, protocol or provider is introduced.

## Historical 0.70 stale-navigation candidate checkpoint

**0.70 is not yet accepted.** A superseded ordinary Open Read may still populate
an exact inactive buffer, but must not leave a canceled Text line jump on a newly
created document. The candidate guards creation of that ordinary `jump_to` by
current navigation identity and the absence of a language-navigation target.
A later Files or Recent ordinary Open must match fresh ordinary-Open caret
behavior for the same bytes. Existing valid background jumps, dirty drafts,
Undo and location-history boundaries remain required.

The old 0.69 production-frame negative control observed scalar **10** after the
stale Text line-2 Read was cached and the file reopened from Files or Recent;
a fresh ordinary Open of identical bytes produced scalar **39**. Dirty Undo and
history remained safe. Its two positive controls passed and its stale-line
rejection assertion failed intentionally. This is a supplied-event production-
frame result, not a prior native GUI observation.

The [offline contract](FIND_AND_RETURN.md#stale-navigation-isolation-070-candidate)
and [phase report](TEST_REPORT_PHASE70_STALE_NAVIGATION.md) distinguish the
candidate's source-derived, frame, normal-agent, native and Windows evidence.
The final **0.70.0** local Rust workspace/all-target/all-feature aggregate passed
**2,203/0/71 across 50 suites**, with formatting and strict Clippy. Python passed
**806 total / ten skipped / zero failed suites across 29 suites**. Current
Cargo-JSON-selected default-feature artifacts passed Browse v7 **74 fields /
17 operations**, Structure **45/11**, Scoped Search **57/10** and Search **57/12**.
Browse retains exact v6/v5/v4 projections and adds one controlled normal-client
Read completion; **17/17 pure parser checks** also passed. Structure uses the
explicit mock. These are source-bound local proofs; canonical compile drivers
were not run locally. Exact-source CI, all three packages and native validation
remain pending, and **0.70 is not accepted**.

The first corrected-source focused **5/0/0** run and early passing v7 process proof
still reported 0.69.0; they remain distinct from final 0.70 evidence. Initial
proof-development failures are preserved: an old schema-version test literal,
a private-field accessor compile error, and an overstrict new Back observer.
The independently reviewed observer correction models entire Back/Forward stacks
and ordinary duplicate-departure coalescing. It changes no production guard and
does not establish a production history defect. The process journey activates
Files first, then reuses the document through Recent; independent Recent-first
activation is covered by the focused frame regression.

<a id="current-069-files-search-as-you-type-candidate"></a>

## Accepted 0.69.0 bounded baseline

The [public source revision](https://github.com/LLLLimbo/cedar-ide/commit/2a1b50a2e220b910251b4afdbe0f8a83f3915303)
(tree `13719ea38dbb7cbbf408be3017fca9e491b01679`) passed all **four jobs** in
[CI 38458861975](https://github.com/LLLLimbo/cedar-ide/actions/runs/38458861975)
and all **three exact-source package audits**. Linux recorded **2,200/0/71** and
Windows **2,138/0/114** (passed/failed/ignored), each across **50 suites**. All
**24 new Files checks** passed. Browse v6 retained **64 strict typed fields / 16
operations**, including the complete prior 57-field contract and original
fourteen-operation prefix, exactly two added FindFiles and zero automatic Reads.

The bounded Trust-off Linux native journey passed typing without Enter,
Directory scope changes, explicit Preview, ordinary Open/Back, dirty Undo and
cancel/reopen, with timing and independent integrity/visual checks passed.
A result click replaced the planned Down step; native Down entry was therefore
**not measured** in that journey. Native pixels do not measure the fixed 250 ms
admission interval or backend operation counts. No broad IME/CJK,
Windows/macOS GUI, real SSH or positive File Structure/JDT GUI acceptance follows.

Linux Java Restart cleanup reported **`grace_expired`, signal 9**. It is a forced
cleanup result, not an all-graceful shutdown claim. Native owned-frontend exit
(code zero) and window absence are separate evidence and do not prove descendant
reaping. The original 0.68 startup observation remains incomplete, with no product
startup-defect inference. All earlier failures retain their classifications.

The [phase 69 report](TEST_REPORT_PHASE69_FILES_LIVE_SEARCH.md) adds an accepted
follow-up and preserves its complete original preparation record. Local versioned
Rust **2,200/0/71**, Python **805 total / ten skipped / zero failed suites across
29 suites**, strict Clippy/formatting and independent input review passed.
Selected-artifact Structure **45/11**, Browse v6 **64/16**, Scoped Search **57/10**
and Search **57/12** proofs passed (fields/operations); Structure uses a mock.
The first versioned welcome-label assertion failure, earlier focused failures,
and disk-only preview-observer failure remain failed at their original scope.
Their later bounded corrections and passing reruns do not rewrite them.

## Accepted 0.68.0 bounded baseline

The [public source revision](https://github.com/LLLLimbo/cedar-ide/commit/a525c547aeb2f1fc1d80d3d872801c1f4d986963)
passed all **four jobs** in
[CI 38444382093](https://github.com/LLLLimbo/cedar-ide/actions/runs/38444382093)
and all **three exact-source package audits**. Linux recorded **2,176/0/71** and
Windows **2,114/0/114** (passed/failed/ignored), each across **50 suites**.
Browse v5 retained **57 fields / 14 operations / zero added current-file Find
operations**; retained Structure, Scoped Search, Search and platform/JDT proofs
passed within their individual scopes. The local versioned Rust aggregate,
Python **804 total / zero failures / ten skipped across 29 suites**, strict
Clippy, formatting and independent input review also passed.

The second bounded Linux native journey, with Trust off, passed visible
whole-selection seeding, F3/Shift+F3, labelled Next/Previous, Escape and Back to
the departure, no-hit F3 stability, dirty Undo/Redo and normal discard/quit.
Startup **18.209 seconds**, connected cases to quit initiation **245.663 seconds**
and owned frontend exit **13.508 seconds** after quit passed their fixed bounds.
Separate integrity and visual review passed for the matching package and one
generated short ASCII/Unicode fixture. Exit code zero and window absence support
owned frontend cleanup, not descendant reaping.

The first native attempt remains **incomplete**: its startup observation exceeded
the budget before any Connect or functional input. This is not evidence of a
product startup defect, and the second result does not rewrite it. Native
10,000-match-cap behavior, raw selection affinities, backend operation counts,
adjacent-frame latency, large buffers and tiny viewports remain unmeasured.
There is no Windows/macOS GUI, real SSH, broad IME/CJK or positive native
File Structure/JDT claim. The earlier native Ctrl+F12 and other historical
failures retain their classifications.

The [phase 68 report](TEST_REPORT_PHASE68_CURRENT_FILE_FIND.md) adds an accepted
follow-up while retaining the original preparation text unchanged, including its
first aggregate failure and intended strict-input contract correction. Historical
pending statements describe that earlier checkpoint; the terminal result above
supersedes it without treating old local checks as new release evidence.

## Accepted 0.67.1 bounded baseline

The [public source revision](https://github.com/LLLLimbo/cedar-ide/commit/e5f94aa197c5ae676572a0e68cc25c22c9d3bea6)
passed all **four jobs** in
[CI 38431261412](https://github.com/LLLLimbo/cedar-ide/actions/runs/38431261412)
and all **three package audits**. Linux recorded **2,160/0/71** and Windows
**2,098/0/114** (passed/failed/ignored), each across **50 suites** and all
**32 File Structure frame checks**. The Linux normal-agent/mock Structure proof
passed its strict **45-field / 11-operation** receipt; retained Browse,
Scoped Search, Search and existing platform/JDT checks passed within their
separate scopes. Local Python **803 total / zero failures / ten skipped across
29 suites**, strict Clippy, formatting and independent source/input review
also passed.

The shipping Linux native journey kept Trust off and covered the visible
**Tools → File Structure…** menu, unavailable-provider explanation, local
filter, Escape, reopen and dirty-selection Undo/Redo. Startup **25.38 s**,
connected cases to quit initiation **220.92 s**, and quit **12.62 s** passed
their fixed timing bounds; artifact and fixture integrity checks passed.
Typing reached the focused filter without an extra focus click, but the
**11.662 s / 13.587 s** next-input gaps do not establish adjacent native frames.
Immediate typing is covered separately by supplied-input frame tests.

This is bounded acceptance led by CI. Positive structure is established by
headless frames and the Linux mock-provider process proof; native coverage is
only the Trust-off unavailable-provider journey. **Positive native symbols,
real-JDT File Structure GUI and native Ctrl+F12 delivery remain unmeasured.**
No Windows/macOS GUI, real SSH, broad IME/CJK or descendant-reaping native claim
follows. See the [phase report](TEST_REPORT_PHASE67_FILE_STRUCTURE.md).

## Preserved 0.67.0 results and native entry failure

Public commit
[`212f885d59eeddcb2f12628e0b9eb45991271dca`](https://github.com/LLLLimbo/cedar-ide/commit/212f885d59eeddcb2f12628e0b9eb45991271dca)
passed all **four jobs** in
[CI 38410333307](https://github.com/LLLLimbo/cedar-ide/actions/runs/38410333307)
and all three package audits. Linux recorded **2,145/0/71**, Windows
**2,083/0/114**, with **50 suites and 17 File Structure actual-frame checks per
platform**. The Linux normal-agent/mock proof passed **45 fields / 11 Client
operations**. Retained Browse **50 fields / 14 operations**, Scoped Search
**57 fields / ten operations**, Search **twelve operations**, and the existing
platform/JDT checks passed within their separate scopes.

The bounded Linux native Trust-off journey stopped at its first Ctrl+F12 attempt:
no File Structure popup was visible. The unavailable explanation, filtering,
reopen, Escape restoration and Undo/Redo were not demonstrated in that journey.
A separate complete, loss-free instrumented local probe recorded **520 finished
frames, zero F12 presses and one release** at Cedar’s raw-input hook. This shows
only the observed raw-hook boundary; it proves neither upstream interception nor
the cause or input batch of the earlier shipping failure. Native shortcut and
positive-provider acceptance remain unestablished.

The earlier rapid-reopen production defect and test-completion failure, their
bounded corrections, original local checks and diagnostic limits remain in the
[phase report](TEST_REPORT_PHASE67_FILE_STRUCTURE.md). Those earlier results did not validate 0.67.1; its later bounded acceptance
is recorded separately above. See the
[current usage contract](FILE_STRUCTURE.md).

## Accepted 0.66.2 bounded baseline

Public commit
[`a0ecbdbe27577ef83a60eee1b8af88edb0c68f35`](https://github.com/LLLLimbo/cedar-ide/commit/a0ecbdbe27577ef83a60eee1b8af88edb0c68f35)
passed all **four jobs** in
[CI 38392603729](https://github.com/LLLLimbo/cedar-ide/actions/runs/38392603729)
and all **three exact-source package audits**. Linux recorded **2,128/0/70** and
Windows **2,066/0/114** (passed/failed/ignored), each across **50 suites**, including
all **17 preview tests** on both systems. Strict Browse passed **v4, 50 fields /
14 operations**, retaining all 38 prior field assertions and the original
12-operation prefix. Scoped Search **ten operations**, Search **twelve
operations** and both platform JDT proofs were verified.

The fresh bounded Linux native journey passed continuous follow in both Text
surfaces, the previously failed bottom B160 preview, ordinary Open/Back, dirty
selection and Undo/Redo, and Escape/hide/reopen. Startup was **14.020 seconds**;
quit began **491.046 seconds after visible Connected** and exit code **0** arrived
**17.309 seconds** later. The bounded functional and timing gates passed.
Native pixels do not prove the **150 ms** settlement interval, request ledgers or
raw caret affinities; those remain separate deterministic/process evidence.
Windows/macOS GUI, real SSH, arbitrary viewports and broad CJK/IME remain outside
this acceptance.

0.66.2 changes the test-only caller helper to recognize the exact `browse.rs`
basename with either path separator. The 0.66.1 production repaint implementation
is byte-for-byte unchanged. The fresh Windows results establish the portability
correction’s pass without rewriting the earlier failure’s unlogged runtime cause.
The 0.66.0 native bottom-preview failure, 0.66.1 Windows caller-count failure and
never-launched native check, 0.65.0 glyph stop and 0.64.0 strict timing failure
remain preserved below and in their phase reports.

## Preserved 0.66.2 local correction checks

Independent review, **17/17 focused checks**, the full Rust aggregate
(**2,128/0/70 across 50 suites**) and Python aggregate (**793 total / zero
failures / 10 skipped across 28 suites**), strict Clippy and formatting passed.
Both negative controls retained their intended failures. The
Cargo-JSON-selected app and matching default-features normal agent passed
strict Browse **v4, 50 fields / 14 operations** with all 38 prior assertions and
the 12-operation prefix, plus retained scoped search **ten operations**. The
canonical release drivers were not run locally. These local checks are separate
from the accepted exact CI, package and native evidence above.

## Preserved 0.66.1 CI failure

Public 0.66.1 commit
[`f972998b212889a6cb8d2e900e2c077a9c3af165`](https://github.com/LLLLimbo/cedar-ide/commit/f972998b212889a6cb8d2e900e2c077a9c3af165)
failed three new Windows repaint tests in
[CI 38388794011](https://github.com/LLLLimbo/cedar-ide/actions/runs/38388794011):
the caller count was zero instead of one despite a zero-delay due-frame repaint
and valid internal B160 preview state. The raw cause filename was not logged;
a separator mismatch in the `/browse.rs` suffix filter is a source-level
hypothesis. That run did not establish a Windows fix. Linux passed
**2,127/0/70 across 50 suites**, its runtime proofs and both package audits.
Windows stopped with an observed **1,421/3/57 prefix across eight suites**;
its release/runtime proofs and package were absent. The dependent Java signature
matrix was entirely skipped, including both producer jobs. **0.66.1 native
validation was never launched**; it remained held at the Windows CI gate.

## Preserved 0.66.1 local correction checks

The unaccepted 0.66.1 candidate contains a narrow repaint correction for
continuous Project Text preview. A controlled actual-frame diagnostic found that
both Text surfaces could fulfill a due preview from an open draft after painting
the pending state without requesting another frame. The correction requests
one repaint when authorized due fulfillment synchronously produces a visible
preview, preserving the existing preview contract. Final-version focused Rust
checks passed **16/16** (11 retained and five new repaint regressions). The final
workspace/all-target/all-feature/locked Rust aggregate passed **2,127/0/70 across
50 suites**, and Python passed **793 total tests / zero failures / 10 skipped
across 28 suites**. Independent review, strict Clippy and formatting passed.
The final Cargo-JSON-selected current app and matching default-features normal
agent passed strict Browse **v4, 50 fields / 14 operations**, retaining all
38 prior field assertions and the 12-operation prefix, plus retained scoped
search **ten operations**. The canonical release drivers were not run locally;
these local passes did not override the later Windows CI failure. A stale version
expectation in process fixtures stopped the first scoped-search attempt; narrow
version maintenance and the final full/strict/runtime reruns passed before
sealing.

At that preparation checkpoint, the accepted bounded baseline was **0.65.1**. The source/frame
diagnostic is separate from the 0.66.0 native observation and does not establish
that run's exact internal cause. See the
[phase report](TEST_REPORT_PHASE66_CONTINUOUS_PREVIEW.md) and
[packaged guide](FIND_AND_RETURN.md#continuous-project-text-preview-066-candidate).

## Preserved 0.66.0 CI, packages and native failure

Public commit
[`84d7393f3ace6ef153bed26994f480ccd9bf1abe`](https://github.com/LLLLimbo/cedar-ide/commit/84d7393f3ace6ef153bed26994f480ccd9bf1abe)
passed all **four jobs** in
[CI 38382019936](https://github.com/LLLLimbo/cedar-ide/actions/runs/38382019936)
and all **three exact-source package audits**. Linux recorded **2,122/0/70** and
Windows **2,060/0/114** (passed/failed/ignored), each across **50 suites**. Strict
Browse passed **v4, 50 fields / 14 operations**, retaining all **38 prior field
assertions** and the original **12-operation prefix**.

The bounded Linux native journey **failed**: after bottom FIND RESULTS Down
selected already-open B160, “Preview requested...” remained visible
**37.223 seconds** later before quit. Modal continuous follow, ordinary Open/Back,
dirty selection and Undo/Redo passed; bottom Up and bottom Escape/hide/reopen were
not run. Startup was **16.444 seconds**; quit began **348.689 seconds after visible
Connected**, exited **0** after **9.907 seconds**, and the window was absent after
**10.027 seconds**. Timing, safe cleanup and source/package/fixture integrity
passed separately from the failed functional gate.

The Linux Java proof's spontaneous-diagnostic wait timed out at **60 seconds**;
the existing allowed recovery refresh then matched. Java restart cleanup used
`grace_expired` / **SIGKILL (signal 9)**, so both stops were not graceful. The
earlier local history/focus fixture failures, first aggregate failure and strict-check
corrections remain in the [phase report](TEST_REPORT_PHASE66_CONTINUOUS_PREVIEW.md).
None of these 0.66.0 results substitutes for the separate correction-candidate evidence.

## Accepted 0.65.1 bounded baseline

Public commit
[`bc65427bb1fc2c0da3b7391c65a8340ed0619db4`](https://github.com/LLLLimbo/cedar-ide/commit/bc65427bb1fc2c0da3b7391c65a8340ed0619db4)
passed all **four jobs** in
[CI 38371007337](https://github.com/LLLLimbo/cedar-ide/actions/runs/38371007337)
and all **three exact-source package audits**. Linux recorded **2,111/0/70** and
Windows **2,049/0/114** (passed/failed/ignored), each across **50 suites**. Both new
CJK tests and all **27 Project selection tests** passed; strict Browse retained
**v3, 38 fields / 12 operations**, scoped search **ten operations** and Search
**twelve operations**. Required JDT checks also passed.

The fresh bounded Linux native journey passed a readable `src/层/Target.rs`
path, Tree/Flat current-file selection, Down/Up, dirty-selection and Undo/Redo
preservation, and retained Find state. Startup was **30.994 seconds** against
90 seconds. Quit began **303.840 seconds after visible Connected**, before the
550-second wind-down and 600-second case limits; exit code **0** arrived after
**12.579 seconds**, and the window was absent after **12.703 seconds**, within
the 120-second shutdown bound. The bounded functional and timing scope passed.
Native cancellation, Windows/macOS GUI, real SSH, arbitrary viewports and general
CJK/IME coverage remain unverified.

The narrow CJK correction uses the existing bounded local font loader only after
accepting current, validated result paths/snippets. It adds no download,
installation, dependency, protocol or backend operation. The retained explicit
Project action uses one shared List slot, at most one known-parent List in Flat
or 32 ancestor Lists in Tree, and one fixed 30-second intent deadline. It requires
current visible-row identity, preserves dirty selection/Undo/Find, and adds no
Read, recursive scan or automatic follow. Alt+1 remains zero-request focus.

The preceding 0.65.0 commit
[`ec8428a020acc58e47eb5c722de331fcb2b55a03`](https://github.com/LLLLimbo/cedar-ide/commit/ec8428a020acc58e47eb5c722de331fcb2b55a03)
passed [CI 38365574615](https://github.com/LLLLimbo/cedar-ide/actions/runs/38365574615)
and three packages, but its native journey stopped at a box in the same CJK path.
UTF-8 remained intact; core Project selection, dirty-selection, Down/Up and Undo
were **not run**; safe exit was **0**. The installed font alone had not triggered
search-result fallback. That failed journey, the local epoch-baseline fixture
failure, immediate-Cancel proof correction, preventive IME lifecycle refinement
and distinct local/process evidence remain in the
[phase report](TEST_REPORT_PHASE65_PROJECT_REVEAL.md). Its 0.65.0 Linux Java restart
used `grace_expired` / **SIGKILL (signal 9)**; satisfying the cleanup predicate
does not mean both Java stops were graceful.

## Accepted 0.64.0 bounded functional baseline

Public commit
[`cf6fbb4c3b7b3149a1c345a1485ccbb9b50ff49e`](https://github.com/LLLLimbo/cedar-ide/commit/cf6fbb4c3b7b3149a1c345a1485ccbb9b50ff49e)
passed all **four jobs** in
[CI 38354618490](https://github.com/LLLLimbo/cedar-ide/actions/runs/38354618490)
and all **three exact-source package audits**. Each platform ran **50 suites**:
Linux **2,082 passed / zero failed / 70 ignored** and Windows
**2,020 passed / zero failed / 114 ignored**, including **19 new Recent
Files/control cases**.

The bounded Linux native run passed essential functional and integrity checks.
Quit initiation was **20.436 seconds late**, so the **strict timing gate failed**.
This is an accepted bounded functional baseline, not an all-gates pass. No later
native rerun is claimed. Windows/macOS GUI, real SSH, arbitrary viewport behavior
and IDEA parity remain outside the established result. The
[0.64 report](TEST_REPORT_PHASE64_RECENT_FILES.md) preserves its preparation
failures and separates local, process, CI, package and native evidence.

The following 0.64 preparation record predates that checkpoint. Its then-pending
statements describe that earlier stage and do not validate later candidates.

## Preserved 0.64 Recent Files preparation

**At this preparation checkpoint, 0.64.0 was an unaccepted candidate and the
latest accepted bounded baseline was 0.63.3.** Recent Files is limited to 64 settled, known-revision active file paths
in the current connection, with 256 KiB total path bytes and 4,096 bytes per path.
Closing a tab retains its path without retaining closed text or caret state.
Clear, disconnect, reconnect, workspace change and restart clear the list; idle
frames cannot repopulate it. Open dirty buffers are reused with zero Read;
closed paths use ordinary Open/Read with its ordinary default caret position.
Recent Files stores and restores no closed caret or selection.

The entry shortcut is Ctrl+E on Windows/Linux and Cmd+E on macOS, preserving
macOS Ctrl+E. The UI provides a local filter, Open/Closed state, This connection
scope and Clear. No preview, edited-only switch, index, protocol extension,
persistence or parity claim is included. The version-2 Browse receipt appends
Recent Files checks after the preserved seven-operation prefix, for eight
operations total. See the
[candidate report](TEST_REPORT_PHASE64_RECENT_FILES.md) and [guide](RECENT_FILES.md).

The first new process attempt failed when a test assumed line 1, column 1 but
observed line 4, column 1. This failure remains recorded. The pinned egui editor's
ordinary fresh Open defaults to the end of the file. Actual-frame controls then
passed with Recent and ordinary Open producing the same settled cursor and
selection for identical bytes and for changed shorter bytes, using a new
document and current revision without restoring historical caret state. The
production Open behavior was unchanged. The process proof now binds its expected
caret to the exact ordinary-Open baseline. An earlier preparation checkpoint
passed 18/18 tests; a subsequent checkpoint passed 19 focused tests and the full
eight-operation Browse runtime proof before the final version bump. These
successes do not substitute for checks of the final 0.64.0 artifacts.

### Preserved final 0.64.0 local results

The final Linux workspace/all-target/all-feature/locked aggregate passed
**2,082/0/70 across 50 suites**, including app **1,366/0/53** and **19 new Recent
Files/control tests**. The updated Python aggregate reported **791 total tests /
zero failures / 10 skipped across 28 suites**. Strict Clippy passed. The first
strict Clippy attempt failed on a test fixture's reassignment
after default construction; equivalent direct initialization fixed that lint
without production changes or a waiver. The failed attempt remains recorded.
The final formatting check passed.

Final-version normal-agent proofs passed their strict validators with the current
app selected from Cargo JSON and the matching current agent: Browse uses the
new exact **eight-operation / 29-field version-2** contract, while scoped search
retains **ten operations**. These are direct local process proofs; the canonical
compile/release pipeline was not run locally. Exact-commit CI, all three
exact-source package audits and new bounded native acceptance remain pending.
Earlier seven-operation historical Browse claims are unchanged.

A prepublication documentation gap was corrected in the three bundle builders:
they now include the Recent Files guide, with bounded validation of nested
relative guide links and public repository URLs for reports. The focused package
suite reported **119 total tests / zero failures / one skipped**, separately from
the pending exact-source package audits. Three new corruption tests initially
hit the size guard after changing file length; same-length mutations corrected
the fixtures to exercise the intended SHA boundary without weakening validation.
Those failures remain recorded. Application Rust, Cargo and workflow bytes were
unchanged, so the existing Rust evidence is retained without claiming a new run.

## Accepted 0.63.3 bounded browsing-shell baseline

Public commit
[`5b6af6c51bfa8e460fc4b4b6367d09d744b1df78`](https://github.com/LLLLimbo/cedar-ide/commit/5b6af6c51bfa8e460fc4b4b6367d09d744b1df78)
passed all **four jobs** in
[CI 38346482830](https://github.com/LLLLimbo/cedar-ide/actions/runs/38346482830)
and all **three exact-source package audits**. Each platform ran **50 suites**:
Linux **2,063 passed / zero failed / 70 ignored** and Windows
**2,001 passed / zero failed / 114 ignored**.

A fresh bounded Linux native journey observed functional Project Tree focus
after Alt+1 without inserting `1` into the dirty draft, retained Find state,
Ctrl+J focus transitions, explicit Preview, ordinary Open, Back and Undo, and
Workspace/Tools menu Escape. These observations establish that bounded native
journey separately from headless input and process proofs. They do not establish
Windows/macOS GUI, real SSH, arbitrary viewport behavior or full IDEA parity.

The earlier native failures, the ambiguous 0.63.2 Flat-mode observation, missing
original evidence and diagnostic timing/setup limitations remain below. The new
accepted journey neither reconstructs missing originals nor proves that earlier
runs delivered the same native input batches.

The following 0.63.3 preparation record predates its accepted checkpoint. Its
then-pending CI, package and native statements describe that earlier point;
they do not make 0.63.3 pending again or validate 0.64.

## Preserved 0.63.3 Alt+1 companion preparation

**At this preparation checkpoint, 0.63.3 was a correction candidate; the latest
accepted bounded baseline was 0.62.2.** It addresses only the Project shortcut's exact clean input sequence:
Alt+Num1 followed by a single `Text("1")` companion. The companion is consumed at
validated shortcut admission, before an editor can receive it. A same-held-key
repeat fence may be armed only after the final guarded Project focus succeeds.
An admitted request that later fails its focus guard does not arm that fence.
Release and repress require a new admission; they cannot reuse old authority.

Unrelated or multiple Text events, Paste, IME, AltGr and competing mixed input
remain subject to the existing rejection rules, with their original input
preserved. The key-only Alt+1 route, Space and Ctrl/Cmd+J retain their existing
contracts. The correction adds no observer, backend operation, protocol,
dependency, search option or execution permission.

A fresh local-only instrumented Linux diagnostic recorded Alt+Num1 and an
accompanying Text-kind event, no admitted shortcut, and an editor version change
from 5 to 6; the selected generated draft visibly became `1`. Raw Text payloads
were deliberately not logged. Inspection of the pinned input adapter confirms
that Alt can accompany printable Text on this Linux path. Together these
establish a mechanism in this fresh diagnostic, not the exact event batches in
earlier native attempts.

Tools were closed; the planned Find baseline and Down/Up and Ctrl+J checks were
not run. The complete loss-free trace recorded zero dispatches after its baseline.
The final functional action occurred 23.654 seconds before the 300-second
deadline, but quit was requested 66 milliseconds after it and the owned process
exit followed 18.122056 seconds later. That timing limitation is retained.

The diagnostic's **55 tests** and successful build were preparation evidence
only. They are not shipping acceptance.

### Final 0.63.3 local validation

The final local Linux workspace/all-target/all-feature/locked aggregate passed
**2,063 passed / zero failed / 70 ignored across 50 suites**, including app
**1,347/0/53**. The focused Access selection passed **40 tests**: six new Alt+1
test functions and 34 retained Access/Space tests. Strict Clippy, formatting and
**779 Python tests across 28 suites** passed.

Actual-frame positive cases cover Tree and Flat Project views, hidden and visible
Find, retained dirty full selection, Undo and location history, and Tree Down/Up
without added operations. Negative cases cover the exact ordered Text boundary,
rejected input remaining in the live event vectors, repeats and release,
blocked admission and final focus invalidation. The supported root Search
fixture proves retained query/result state; it does not prove scoped-search
execution. The initial run of all six new functions failed during unsupported
scoped-search setup because the fixture did not advertise that capability. The
failure preceded the correction assertions; the original failure record remains
separate from the passing run with supported setup.

The current app executable selected from Cargo JSON and matching current normal
agent passed the unchanged strict Browse and scoped-search validators with
exactly **seven/ten operations** and **zero added shell-transition or movement
operations**. These direct process proofs are separate from release packaging;
the canonical release/full-CI driver was not run locally. Exact-commit CI, all
three exact-source packages and a fresh bounded native journey remain pending.
See the [candidate report](TEST_REPORT_PHASE63_BROWSING_SHELL.md).

## Preserved 0.63.2 pointer lifecycle checkpoint

**At this checkpoint, 0.63.2 remained a correction candidate**; the latest
accepted bounded baseline was **0.62.2**. A controlled actual-frame journey established that Find Results
accepts a clean owned pointer gesture but rejects it when a standalone IME Enabled
or Disabled lifecycle notification arrives while the pointer is held, even though
egui reports the actual button click. Preedit and Commit controls were also rejected.
All cases preserved the dirty draft, selection, Undo, location history and results,
and added no operations. This establishes a classifier limitation; it does not
identify the events delivered during the native observation below.

The implemented alignment concerns pointer eligibility only. Lifecycle notifications
may accompany an otherwise clean owned gesture when no composition was already
active. Text, paste, keys, Preedit and Commit, including empty composition events,
remain competing input. Keyboard activation and shortcuts retain their existing
strict original-event rules. Raw input and all exact view/document/session,
response ownership and frame guards remain authoritative.

Local correction checks passed **47 focused tests**, then **2,057/0/70 across
50 workspace suites** (app **1,341/0/53**), strict Clippy and **779 Python tests
across 28 suites**. The exact public
[0.63.2 source](https://github.com/LLLLimbo/cedar-ide/commit/7850e557b6aaff647d1e71e31b1e239d271655d4)
then passed all four jobs in
[CI 38338899789](https://github.com/LLLLimbo/cedar-ide/actions/runs/38338899789)
and all three exact-source package audits. Linux recorded **2,057/0/70** and
Windows **1,995/0/114**, each across **50 suites**. These results do not complete
native acceptance or transfer to 0.63.3.

### Preserved 0.63.2 native boundary

The exact shipping package connected with execution trust off. Workspace and
Tools menu Escape, opening Find Results from the header, filename discovery/Open,
a dirty draft and scoped two-hit search were observed. Alt+1 retained Find state,
but the Flat-mode journey had no functional witness of Project focus. Testing
stopped with an **ambiguous observation, not a demonstrated product failure**.
Ctrl+J, Preview, same-row double-click Open, Back and Undo were not run. Normal
exit returned code 0, the owned process was reaped and checked fixture, package
and source bytes were unchanged. The later diagnostic used a different prepared
state and cannot retroactively identify this shipping run's input batch.

### Preserved 0.63.1 native boundary

The [0.63.1 source](https://github.com/LLLLimbo/cedar-ide/commit/fdb6e1d82cd90b4d1128920fabfdb1bb7841fd81)
uses the narrow menu Escape correction. [CI 38331547998](https://github.com/LLLLimbo/cedar-ide/actions/runs/38331547998)
passed all four jobs and all three exact-source package audits. Linux recorded
**2,051/0/70** and Windows **1,989/0/114**, each across **50 suites**, including
all six new Escape cases and the retained process proofs. These headless results
remain separate from incomplete native acceptance. Its first connection observation ended
before the startup deadline and did not establish a result. A second, correctly
bounded window-targeted input attempt remained Not connected through the deadline.
Neither exercised menu or later functional cases; neither establishes a backend
failure or actual widget click delivery.

A subsequent documented desktop-input journey connected to the generated fixture
with execution trust off. Both Workspace and Tools opened and closed with Escape.
Filename discovery, opening the generated origin, a dirty draft, scoped two-hit
search and selection restoration after modal Escape were observed. A later single
Find Results click did not visibly open the bottom panel after a settled observation,
so testing stopped. Project/Find/editor focus and the subsequent Preview/Open/Back/
Undo sequence were not completed. Normal Discard and quit returned frontend and
wrapper exit code 0, and original fixture, package and source bytes remained intact.
The input-route difference and unknown native event cause remain explicit; a new
candidate must pass its own exact-package native checks.

Some original prelaunch evidence from the 0.63.1 attempt was overwritten by a
later attempt. A matching image survived in later duplicate captures, but that
does not reconstruct the missing original files. The available original evidence
and the separate complete later records remain distinct; no restoration of the
missing originals or stronger native conclusion is claimed.

## Preserved 0.63.1 correction and blocked predecessor

**At this checkpoint, 0.63.1 was a correction candidate; the latest accepted
bounded baseline was 0.62.2.** It closes only the currently owned Workspace or Tools menu when the
existing generic Escape handler receives the key. Earlier navigation, modal and
language Escape owners retain their existing priority. Without an owned menu,
the previous generic dismissal behavior remains. It adds no backend operation,
protocol, dependency, execution permission or new UI feature.

The preceding public [0.63.0 source](https://github.com/LLLLimbo/cedar-ide/commit/bf912c834faf7685ef13437fbc3ad1b20d669a8c)
and [CI 38327699936](https://github.com/LLLLimbo/cedar-ide/actions/runs/38327699936)
remain a distinct checkpoint. All four CI jobs and all three exact-source
packages passed. Linux recorded **2,045/0/70** and Windows **1,983/0/114**
(passed/failed/ignored), each across **50 suites**. Both platforms passed five
shell, 31 workspace-access and 24 retained row-continuity cases. The strict
Browse **22-field/seven-operation** and scoped **57-field/12-case/ten-operation**
receipts reported zero added shell operations; the retained 12-operation Search
proof and both real-JDT producer proofs also passed. These headless results do
not complete native acceptance.

Its fresh Linux native attempt connected with
execution trust off and showed Workspace/status/Recovery entries. An Escape
request left the menu visible in both the immediate after-image and later
quit-before image. Functional cases stopped immediately; the owned frontend
exited normally with code 0, and the archive, source, package and six generated
fixture files were unchanged. Project/Find/editor focus, Preview/Open/Back,
dirty Undo/Redo and narrow-window native checks were not run.

A separate actual-frame test established a source defect for a delivered Escape:
the key survived the original raw-input hook, the generic shortcut handler
consumed it before egui menu rendering, and the menu remained painted after key
release and an idle repaint. This proves that controlled input path; it does not
prove which native widget received the earlier tool-delivered key. The original
native observation and failed regression remain preserved.

The 0.63.1 local aggregate passed **2,051/0/70 across 50 suites**, including
**1,335/0/53** in the app, all six new cases (the original Escape regression plus five additional cases).
Strict Clippy and **779 Python tests across 28 suites** passed. At that pre-CI
preparation point, exact-commit CI, packages and native checks were pending.
The later passed CI/packages and incomplete native results are recorded in the
[preserved 0.63.1 boundary](#preserved-0631-native-boundary); none was inferred
from 0.63.0.

The following preparation record predates the 0.63.0 native stop; its then-pending
CI/native statements describe that earlier point, not current acceptance.

## Preserved 0.63.0 local preparation record

**At this checkpoint, 0.63.0 had passed its final local checks; release acceptance
was pending. The latest accepted bounded baseline was 0.62.2.** The candidate concentrates the
connected workspace on Project, the editor and Find; groups setup under
Workspace and existing tools under Tools; moves Back/Forward into the tab band;
and adjusts Project/Find/editor focus routes. Recovery, error, dirty-draft and
connection states must remain visible and reachable. It adds no backend,
protocol, search-option or Build/Run behavior.

The final local Linux **0.63.0** workspace/all-target/all-feature/locked aggregate passed
**50 suites: 2,045 passed / zero failed / 70 ignored**, including the app's
**1,329 passed / zero failed / 53 ignored**. Formatting and strict Clippy passed;
**779 Python tests across 28 suites** passed. Actual normal-agent Browse and
scoped-search runtime proofs passed their existing strict validators with exact
**seven/ten operations** and **zero shell-transition operations**, preserving
query and accepted-result identity, grouping, selection, dirty full selection,
Undo and location history within the measured scope. They used the compiled app test
and normal debug agent directly through a bounded runner; the canonical CI
compile/selection/release-driver route was **not executed locally**.

Actual-frame checks assert **40/48** connected-header heights at **780 × 540**
and **1280 × 900** with **13/18** fonts. The **28/34** tools values are title-row
minima; margins, separator and body add height. These checks do not establish
native geometry. The earlier **1,328-pass / 53-ignored** app checkpoint preceded
a stronger ready-navigation test that exposed Ctrl+J cancellation ordering. The
correction and stronger test now pass; the earlier failure remains recorded in
the [historical 0.63.0 report](https://github.com/LLLLimbo/cedar-ide/blob/bf912c834faf7685ef13437fbc3ad1b20d669a8c/docs/TEST_REPORT_PHASE63_BROWSING_SHELL.md).

New exact-commit dual-platform CI, all three exact-source package audits and the
bounded native journey remain pending. These local results do not imply release
or CI acceptance. Windows GUI, real SSH, IDEA parity and comparative performance
remain outside the established evidence.

## Accepted 0.62.2 bounded scoped-search baseline

Public commit
[`91aa84cb79547207ed4deb296a61834df26089ef`](https://github.com/LLLLimbo/cedar-ide/commit/91aa84cb79547207ed4deb296a61834df26089ef)
passed [CI 38282992861](https://github.com/LLLLimbo/cedar-ide/actions/runs/38282992861).
All **four CI jobs** and **three exact-source packages** passed verification.
Linux recorded **2,031 passed / zero failed / 70 ignored**; Windows recorded
**1,969 passed / zero failed / 114 ignored**. Both platforms passed **58 Browse**,
**43 Search** and **13 scoped-search frontend frame checks**.

The normal-agent proofs retained their exact strict contracts: Browse
**17 fields / seven operations**, Search **57 fields / 14 cases / 12 operations**,
and scoped search **52 fields / 12 cases / ten operations**. These deterministic
and process checks establish bounded input, compatibility and operation
contracts; they are separate from observed native pointer delivery and pixels.

A fresh bounded Linux native journey used the exact 0.62.2 desktop package with
execution trust off. It independently observed:

- Single-click selection of a previously unselected result, explicit Preview,
  and genuine operating-system same-row double-click Open in both the shared
  Project Text window and bottom FIND RESULTS tool
- The submitted **Directory `src` / File masks `*.rs`** result set, and group
  collapse and expansion
- Return with Back to the dirty origin, followed by Undo/Redo
- Safe normal exit with observed frontend exit code **0**

The optional native invalid-scope, empty-mask, broader OR-mask, edited-tuple
inactivity, modal-reopen and narrow-geometry checks were **not run**. Existing
deterministic coverage remains evidence of its own scope; it is not a native
execution of those branches. The fresh run does not establish Windows GUI,
real SSH, native backend operation counts, exact selection affinities, descendant
reaping or performance. The earlier Ctrl+Alt+Left/Right native navigation
limitation has not been cleared by this scoped-search journey.

Following an environment change, the original public CI and packages were
recovered and verified. Earlier private native attachments were unavailable;
they were not re-inspected or reconstructed. The fresh Linux journey above is
new, separate evidence, not a recovery or reinterpretation of those observations.

This accepts the stated **0.62.2 scoped-search and row-click correction scope**,
not every roadmap journey or all of B2. The 0.62.0 public CI failure and 0.62.1
controlled cross-widget defect remain below. The exact cause of 0.62.1's earlier
unexpected native Open remains **unknown**: the controlled defect establishes a
reason to fix both surfaces, but does not retroactively diagnose that native
sequence. The new 0.62.2 pass does not erase it.

## Preserved 0.62.2 preparation record

The following preparation results and pending statements precede the accepted
0.62.2 outcome above. They retain their original scope; they do not make the
accepted baseline pending again or supply evidence for the 0.63 shell.

### 0.62.2 row-click correction and acceptance state at preparation

**At this preparation checkpoint, 0.62.2 was a pending correction candidate and
the latest accepted bounded baseline was 0.61.0.** The 0.62.1 public CI and all three packages passed, but acceptance
is blocked by a confirmed cross-widget result-activation defect. Its earlier
native unexpected Open remains separately unexplained, and its narrow native
layout remains unverified. None of those limits is cleared by preparing this
correction.

The candidate adds a row-click continuity guard to both the shared Project Text
window and bottom FIND RESULTS tool. Ordinary Open still requires egui's own
double-click classification and current press/release ownership, plus a prior
accepted clean click on the exact same result in the same view and preview
pane. Identity includes the widget, accepted result source, original row index
and content identity, rather than only a displayed path or current selection.
The shared window applies the same guard to its file and symbol providers.

Group, control and other-row input breaks that continuity, as do rejected or
mixed gestures, foreign or window focus loss, changes to query, filters,
provider or results, changed workspace/session/navigation context, preview-pane
changes and hide/reopen.
Clicking one surface cannot qualify a click in the other. Evidence from before
the original raw batch is kept separate from the current completion, which is
committed only once after an accepted nonsizing response. Repeated layout
passes cannot turn one release into its own preceding click.

The guard tolerates egui temporarily clearing keyboard focus only while the
exact eligible carried row press still owns the native widget and the primary
button remains down. Idle missing focus or focus assigned to a foreign widget
does not qualify for that exception.

Clean held-pointer motion and idle frames retain continuity. A valid carried
release followed by a new press in one batch also remains supported; it is not
rejected merely because the input was coalesced. Malformed or unsupported
multiple-edge gestures cannot establish continuity. An egui triple-click does
not synthesize an Open. These are bounded event schedules, not a claim about
every possible native delivery pattern.

This is a frontend input correction, with no custom double-click timer,
protocol, backend or CI-workflow change. The surfaces retain their own focus,
keyboard and Escape behavior. The scoped-search compatibility, limits and
intentional legacy late-completion behavior remain as described below.

### 0.62.2 local correction checks and remaining gates

The versioned **0.62.2 Linux workspace, all-target, all-feature, locked**
aggregate passed **50 suites, 2,031 tests / zero failed / 70 ignored**.
Strict workspace, all-target, all-feature Clippy with warnings denied also
passed. The selected Python driver, package and export checks recorded
**228 tests: 227 passed, zero failed and one skipped** across ten families.
All **343 upstream dependency records** remain unchanged. The product version
and retained exact-version assertions advance to 0.62.2 without changing the
protocol, dependencies, backend or workflow.

The matching **0.62.2 normal agent** passed the local proofs through their
original strict validators: Browse retained **17 fields / seven operations**,
Search retained **57 fields / 14 cases / 12 operations**, and scoped search
retained **52 fields / 12 cases / ten operations**. Final formatting passed.
These are Linux local results. Fresh exact-commit dual-platform CI, all three
exact-source package audits and a bounded native journey remain separate
acceptance gates; local Python package checks and debug-built normal-agent
proofs do not establish built-package or native acceptance.

### Preserved 0.62.2 preparation results

The final pre-version combined focused suite passed **24 tests / zero failed**:
**14 shared-window Browse cases and ten bottom-tool cases**. The full Search
selection also passed **43 tests / zero failed** against the same production
source. These checks ran before the product-version bump, with the test binary
still labelled **0.62.1**; they are evidence for the corrected local source, not
a new execution of the public 0.62.1 source or final versioned 0.62.2 acceptance.
Formatting passed.
Coverage includes rapid group/other-row sequences, duplicate-content row
identity, both cross-surface directions, interrupted and rejected gestures,
lifecycle changes, genuine same-row doubles, held motion, carried release/press,
repeated layout passes and actual triple-click classification.

Earlier preparation attempts remain failures: the first focused selection had
**three passed / four failed**, and a bottom characterization attempt failed
compilation before any test ran. An intermediate combined run had **20 passed /
two failed** in cross-surface fixture setup. Correcting those fixtures produced
an initial 22-test pass; the earlier runs are not relabelled as passing aggregates.

The subsequent retained Search run recorded **41 passed / one failed**. Its
unchanged dirty-buffer double-click test exposed a real compatibility regression
in the new guard: the first row click was accepted, but egui temporarily cleared
keyboard focus during the matching, owned second press. The release then lost
the prior completion solely because focus was absent. The correction preserves
that exact clean held press with current native ownership;
foreign/window focus, mixed input and context changes remain invalidating. The
equivalent shared-window condition was corrected preventively; its tested
geometry did not reproduce the bottom tool's transient focus state. The earlier
22-test pass did not clear this retained regression. The final 24-test focused
and 43-test Search runs include the correction and pass the unchanged retained
test. A later preparation fixture also failed by asserting history before
Open's next-frame commit; settling that frame, as the existing tests do, yielded
the final focused pass. These failed attempts remain part of the record.

Acceptance still requires fresh exact-commit dual-platform CI, all three
exact-source package audits and a fresh bounded native journey using that
candidate's exact package. The native journey
must independently observe group activation, a first click on a previously
unselected result, explicit Preview and deliberate same-row double-click, and
resolve the unverified narrow layout. Passing earlier headless tests or
observing isolated later Preview actions does not supply that evidence.

## 0.62.1 public CI passed; acceptance blocked

Public commit
[`f1021b8c61cc5b526a115b0f60ad9ab4907ba648`](https://github.com/LLLLimbo/cedar-ide/commit/f1021b8c61cc5b526a115b0f60ad9ab4907ba648)
passed [CI 38272343015](https://github.com/LLLLimbo/cedar-ide/actions/runs/38272343015).
All **four jobs**, both strict checks, both complete workspace aggregates and
all **three exact-source packages** passed verification. Linux recorded
**2,007 passed / zero failed / 70 ignored** and Windows recorded
**1,945 passed / zero failed / 114 ignored**, each across **50 suites**.
Both platforms passed all **13 scoped frontend frame checks**, **44 Browse
checks**, the scoped **52-field / 12-case / 10-operation** normal-agent proof
and the retained normal-agent gates. Both independent real-JDT producer jobs
also passed.

The scoped driver's UTF-8 correction passed all ten driver checks on each
platform, including the four output-encoding regressions. Complete scoped
proofs and the post-runtime agent checks were verified on both platforms.
The Windows development, Linux agent and Linux desktop package results belong
to this exact public source. They do not retrospectively repair the failed
0.62.0 run and do not establish native acceptance of 0.62.1.

### Confirmed cross-widget activation defect

A separate actual-frame diagnostic of the shared Project Text window used the
unchanged 0.62.1 production implementation with a test-only addition. It
established three distinct outcomes:

- A group click followed by a click on a different, previously unselected
  result, **60 ms between releases and 24 pixels apart**, produced egui's
  double-click classification and performed ordinary Open. The result selection
  changed from the preceding row to the Unicode row; an unopened-file Read, new
  document and Back entry confirmed the unintended Open.
- The same group-to-result sequence spaced **760 ms** apart selected the result
  without opening it. A subsequent explicit Preview remained Preview, with no
  new document or history entry.
- Two clicks on the same result **30 ms** apart performed ordinary Open, the
  intended positive behavior.

A later actual-frame characterization confirmed the same defect class in the
bottom **FIND RESULTS** tool before changing its production input behavior.
A group click followed by a first click on a previously unselected result
**60 ms** later reported an egui double-click and issued one ordinary-Open Read.
The navigation version advanced; completing that Read produced a second tab
and one Back entry. This is independent evidence for correcting both surfaces.

Pinned egui classifies double-clicks from global pointer-release timing without
requiring the preceding click to belong to the same widget. The shared window
validates the current row's press/release ownership but then trusts that temporal
classification for Open. Retiring the group action does not retire egui's
earlier click timing. The diagnostic therefore establishes a concrete
cross-widget defect; it does **not** identify the cause of the earlier native
three-click sequence below. The first shared-window diagnostic attempt failed
compilation and executed no tests; the corrected test harness produced its
observations above without changing production code. These focused diagnostics
were neither full-suite runs nor native-window tests.

### Preserved native observations and gaps

The bounded Linux native journey used the exact 0.62.1 package with trust off.
It observed directory/mask filtering, Unicode paths, group collapse and retained
reopen, inactive edited and edited-back tuples, explicit resubmission,
validation refusals, shared bottom-tool results, current dirty-buffer Preview,
ordinary Open/Back and a retained Undo/Redo step. Those positive observations
are bounded to the independently observed actions.

**An earlier sequence of three dependent clicks unexpectedly performed ordinary
Open while Preview was intended.** There was no observation between the group,
result and anticipated Preview clicks; the later targets relied on a prior
layout. Individual input-delivery times and exact widget ownership were not
recorded. Product failure versus targeting failure for that particular native
sequence remains unresolved. Later isolated Preview observations do not explain
or erase it. The discrepancy was escalated after the sequence rather than when
first observed, so the original broad functional-pass statement cannot support
combined acceptance.

The attempted **780 × 540** resize did not establish changed Cedar geometry.
An incidental change to an overlapping browser's geometry was restored, and a
later Cedar-specific resize attempt also failed to establish the narrow bounds.
No native narrow-layout pass or failure is claimed. Normal quit established
window and frontend-process absence within the cleanup bound, but the original
journey did not capture an exit code or descendant reaping. Package and fixture
integrity were checked. Pixels do not establish backend request/read counts,
pending races or exact selection affinities; no Windows GUI or real-SSH result
is claimed.

A separate native diagnostic stopped **before connection** because its startup
observation helper reported an absent Cedar window while later inventory found
that window present. The failing inventory and exact per-API timings were not
retained, so the cause is unknown. It performed zero connection, Find, group,
result or Preview actions and provided no functional or narrow-layout coverage.
Its owned frontend closed normally and the wrapper observed exit status **0**;
this does not establish descendant reaping. That stopped run neither reproduces
nor clears the earlier unexpected Open.

## Preserved 0.62.1 preparation record

The following local correction results and pending statements describe
preparation before the public CI and blocked acceptance outcome above. They
remain evidence for their stated local scope and are not 0.62.2 executions.

### 0.62.1 correction and pending acceptance at preparation

**At this preparation checkpoint, 0.62.1 was a pending correction candidate and
the latest accepted bounded baseline was 0.61.0.** The public 0.62.0 run failed at the Windows scoped-search process
driver, as recorded below. Its successful checks and Linux packages do not make
0.62.0 accepted or establish acceptance of 0.62.1. Scoped-search native
acceptance remains on hold pending the corrected exact-source evidence.

The correction is limited to configuring the scoped-search driver's CLI stdout
and stderr as UTF-8, plus regressions that exercise actual non-BMP output under
cp1252 on both success and failure paths. The shared helper, receipt schema,
validation predicates, limits and workflow remain unchanged. Shipping Rust
behavior is unchanged apart from the product-version update. The scoped-search
contract and intentional frontend late-completion behavior remain as described
in the preserved 0.62.0 scope below.

Local correction verification recorded **228 Python tests: 227 passed and one
skipped**, including all ten scoped-driver tests. The cp1252 regressions preserve
exact UTF-8 output bytes for both LF and CRLF, and keep invalid receipts and
subprocess errors as failures. Formatting and source-equivalence checks passed:
the workflow and shared helper are identical, the four Rust proof changes are
version literals only, and all 343 upstream dependency records are unchanged.
These checks do not include a rebuild of unchanged Rust. The 2,007-test Linux and 1,945-test Windows aggregate
passes below belong to 0.62.0. No 0.62.1 strict-check, workspace aggregate,
process-proof, package or native pass is claimed here. Acceptance still needs
fresh exact-commit dual-platform gates,
complete normal-agent proof receipts, all three exact-source package audits and
the bounded exact-package native journey. The original failure is retained;
adding an encoding correction does not retrospectively complete its receipt or
create the missing Windows package.

## Preserved 0.62.0 public CI failure

Public commit
[`4d1f1d160b4d8991c9873ab7f01befff9a40de1f`](https://github.com/LLLLimbo/cedar-ide/commit/4d1f1d160b4d8991c9873ab7f01befff9a40de1f)
was tested by [CI 38269803398](https://github.com/LLLLimbo/cedar-ide/actions/runs/38269803398).
**The run failed and 0.62.0 was not accepted.** Both platforms passed strict
checks, their complete workspace aggregates and release builds. Linux recorded
**2,007 passed / zero failed / 70 ignored** and Windows recorded
**1,945 passed / zero failed / 114 ignored**, each across **50 suites**.
All **13 scoped frontend frame checks** and the retained Browse proof passed on
both platforms.

The Linux scoped-search normal-agent proof passed its strict **52-field /
12-case / 10-operation** contract, and all retained Linux normal-agent proofs
passed. The exact-source Linux agent and Linux
desktop packages were independently verified. These successful Linux outcomes
are preserved separately from the failed Windows process-driver gate.

On Windows, after the strict validator returned, the shared `checked_stage`
helper attempted to print captured stdout. The cp1252 output stream raised
`UnicodeEncodeError` for **U+1F600**. A complete Windows scoped-proof receipt was
unavailable, and the final agent-digest check was not reached. Reaching the
validator return does not establish a verified Windows proof; no such pass is
claimed.

The Windows development package was absent, many later Windows stages were
skipped, and both dependent real-JDT producer jobs were skipped. Windows Maven
and the later independently conditioned report, Search and shipping gates
passed. Those outcomes do not complete all gates or repair the missing proof
and package evidence. There is no accepted 0.62 native result,
Windows GUI result or real-SSH result from this run.

## Preserved 0.62.0 preparation record

The following local results and pending statements describe preparation before
the public CI outcome above. They remain evidence for their stated local scope
and are not 0.62.1 executions.

### 0.62.0 scope and acceptance state at preparation

**At this preparation checkpoint, 0.62.0 was not accepted and the latest accepted
bounded baseline was 0.61.0.**
The new scoped Project Text path has the local evidence below, including a
passing final versioned workspace aggregate, formatting, strict checks and
matching-version normal-agent proofs. Exact-commit dual-platform CI, all three
exact-source package audits and exact-package native acceptance were then pending.
Accepted 0.61.0 evidence does not validate the new filtered-search path. No new
Windows GUI or real-SSH acceptance is claimed.

The shared **Project Text** window and bottom **FIND RESULTS** tool now edit the
same query, **Directory** and **File masks** fields. Submission remains explicit.
Directory selects an exact workspace-relative subtree; empty means the root.
Masks are an optional, semicolon-separated list of at most eight whole-basename
patterns, totaling 256 UTF-8 bytes after interface separator trimming. Matching
is case-sensitive and supports only `*` and `?`, measured in Unicode scalar
values without normalization. Text matching remains a case-sensitive literal
substring of disk text. The complete validation, compatibility, exclusion and
limit contract is in [Scoped Project Text search](SCOPED_PROJECT_TEXT.md).

Protocol 4, AgentInfo schema 1 and existing capability meanings remain
unchanged. A strictly validated optional Hello `scoped_search` version-1
descriptor enables the dedicated `SearchIn` operation. Empty filters continue
to use legacy `Search`, including on a supported peer. Filtered submissions to
absent, null or unsupported positive-version descriptors send zero scoped
requests and never silently widen to a workspace search. Malformed descriptors
reject the handshake. The accepted 0.61 reader remains a compatibility gate.

Traversal begins at the selected directory and tests filename masks before
opening file contents. Independent limits include 20,000 examined entries,
depth 64, a cooperative five-second budget, 4,096 pending directories, 512 KiB
of pending path text, 1 MiB per eligible file and 32 MiB of actual content bytes
read. Results are bounded to 1,000 backend rows, with 500 requested by the
interface, and 512 KiB of compact encoded result JSON. Existing fixed directory,
link/reparse-point, special-file, binary and invalid-UTF-8 exclusions remain
explicit. Incomplete reasons and counters describe the submitted scan; zero
matches in an incomplete scan do not establish absence. Blocking filesystem
calls can outlast the cooperative deadline.

The retained snapshot now records the full submitted query/directory/mask-list/
limit tuple, extension version or legacy mode, workspace, connection generation
and request identity. Editing any field deactivates historical rows and retires
pending preview/Open intent; editing back, including to equivalent mask spelling,
does not restore it. **This intentionally changes late-completion handling for
in-flight legacy `Search` too:** after a query edit, its late completion is
discarded and the prior snapshot stays visible and inactive until a fresh
explicit submission succeeds. Legacy wire messages and search-engine behavior
are unchanged. This is a frontend behavior change, not a claim of complete
0.61 interface equivalence.

Exact-path grouping, selection, bounded scroll retention, hide/reopen and
Open–Back remain part of the retained flow. Preview of an open dirty draft uses
its current buffer with zero backend Reads; ordinary Open reuses that tab and
Undo history without implicit Save. The one-outstanding-search boundary and
existing preview Read limits remain. This is a subset of B2, with no regex,
whole-word or case-insensitive text mode, live/debounced search, persistent
index, broader semantic navigation or claim of IntelliJ IDEA parity.

### 0.62.0 local checks and then-remaining gates

The recorded focused local results are bounded to the tested source state:

- **24 protocol checks passed:** 13 new checks and 11 checks using the accepted
  0.61 reader. **15 Linux backend checks** and **10 client checks** also passed.
- Two new cancellation checks and the retained eligible-operation check passed.
- An earlier run passed **13 new scoped frontend frame checks** before the
  invalid-mask and edit-away/edit-back strengthening. The final integrated app
  run below also passed all 13 with those assertions included. These are
  headless egui checks, not native-window evidence.
- Strict workspace, all-target, all-feature Clippy with warnings denied passed
  against the final versioned source. The final formatting check also passed.
- The matching-version **0.62.0 scoped normal-agent proof** passed its strict
  **52-field / 12-case / 10-operation** contract and **one owned-agent reap**.
  Its earlier pre-version pass is retained separately, not substituted for this run.
- The versioned **0.62.0 Python selection recorded 224 tests: 223 passed, zero
  failed and one skipped**. These checks do not establish Rust aggregate,
  Windows-host, exact-package or native acceptance.

A prior versioned workspace aggregate attempt failed in the app suite, which recorded
**1,278 passed / 13 failed / 53 ignored**. The 13 failures were in retained app
cases; that attempt was not a successful complete workspace aggregate. After adapting
retained fixtures to the actual Tab order with the new filter fields and the
intentional legacy late-completion rule, **all 33 retained Search checks passed**.
An intermediate Browse run had **43 passed / one clipping failure / one
ignored**, with two additional geometry checks still unresolved at that stage.

The final versioned local Linux **workspace, all-target, all-feature, locked**
aggregate passed **50 suites, 2,007 passed / zero failed / 70 ignored**.
The integrated **0.62.0 app suite passed 1,291 tests / zero failed /
53 ignored**. This includes all **13 new scoped checks, 44 Browse checks,
33 Search checks and 27 retained resize checks**. The previously failing clipping
and geometry cases passed in this integrated run. The passing aggregate does not
substitute for exact-source package audits, Windows execution or native
acceptance.

### Preserved 0.62.0 failures and pending verification

An initial bottom-tool Escape fixture assumed an unsupported action. A long
path-group wrapping failure required a production layout fix. A strict Clippy
failure involved a test-only enum name and was corrected. Retained Tab fixtures
needed the new Directory/File masks controls, and a retained Search history
expectation needed the intentional late-completion change described above.
The earlier failures remain part of the record and are not relabelled as passes.

The history geometry fixture now checks the already-recorded current rectangle
against the current clip instead of reconstructing an egui response from the
previous pass. Its same three frames and ownership assertions are retained.
The clipping fixture uses a measured visible positive case at 210 points, then
restores 150 points for the hidden negative case. Strict input and activation
guards are unchanged; the original 150-point hidden-row boundary remains.

The tools-height fixture retains its original 150-point preference. Search
content may overflow that requested allocation. A separate scroll-contained
Run render verifies recovery of the 150-point allocation without a Run action
or backend operations; its operation ledger stays empty. These fixture
corrections and the integrated pass do not turn insufficient-height Search
content into a general usability pass.

The matching-version retained Browse proof also passed its strict 17-field /
seven-operation contract, and Search passed its 57-field / 14-case / 12-operation
contract. These local executions use the ordinary debug-built agent and exact
ignored test harness; the complete outer CI drivers and release packages were
not executed locally.

At this preparation checkpoint, acceptance still required exact-commit dual-platform CI and the Windows
development, Linux agent and Linux
desktop package audits. Native scoped-search acceptance must use the exact
candidate package and independently exercise the filter/result/dirty-draft
journey. No package, Windows or 0.62 native pass was claimed at that preparation checkpoint. The accepted 0.61 native shortcut limitation and
historical insufficient-height boundaries below remain visible.

## Accepted 0.61.0 bounded retained-results baseline

Public commit
[`7ec8e2f13ce82c57185c705929c0c17c61fcbdfa`](https://github.com/LLLLimbo/cedar-ide/commit/7ec8e2f13ce82c57185c705929c0c17c61fcbdfa)
passed [CI 38213817089](https://github.com/LLLLimbo/cedar-ide/actions/runs/38213817089).
All **four jobs** and **three exact-source packages** passed verification,
including the **44 Browse frame tests**, the **17-field / seven-operation Browse
proof**, the retained **12-operation Search proof** and both independent
real-JDT proofs. These headless and package results remain separate from native
GUI evidence.

The bounded Linux native run used the exact accepted package with trust off.
It exercised grouped selection, explicit Space preview, collapse/expand,
historical-position Open into a dirty draft, duplicate hits in an unopened file,
**Ctrl+[ Back / Ctrl+] Forward**, hide/reopen retention, Undo/Redo and explicit
discard on exit. Fixture and package integrity were checked. This accepts the
stated 0.61 retained-results milestone, not every roadmap journey or the new
0.62 filtered-search behavior.

**Ctrl+Alt+Left/Right were attempted with history available but produced no
observed navigation. The cause remains unknown.** Passing bracket shortcuts and
headless alias tests do not turn that native limitation into a pass. This run
does not establish Windows GUI, real SSH, performance, native exact selection
affinities or native backend operation counts. Historical failures and layout
limits remain preserved below.

## Preserved 0.61.0 preparation record

The following 0.61.0 preparation results precede its accepted public
CI/package/native evidence above. Their pending statements describe that earlier
checkpoint; they do not make accepted 0.61.0 pending again or validate 0.62.0.

### 0.61.0 scope and acceptance state at preparation

**At this preparation checkpoint, the 0.61.0 candidate was not accepted.** Focused local tests and normal-agent
proofs passed, and the versioned 0.61.0 local aggregate and strict checks passed.
Both matching-version normal-agent proofs also passed. Exact-commit dual-platform
CI, all three exact-source package audits and native-window
acceptance were then pending. The accepted 0.60.0 baseline below did not validate these new frontend
paths. No new Windows GUI or real-SSH acceptance is claimed.

The candidate keeps one same-session Project Text snapshot, bounded to 500 rows
and 512 KiB of combined path/snippet text. The shared Find and return window and
FIND RESULTS tool use the same accepted source. Exact-path groups follow
first-seen order and preserve original hit identity, including duplicate-looking
rows. A click selects, Space explicitly previews, and Enter or double-click uses
ordinary Open. A collapsed group retains its selected hit but cannot activate a
hidden row. Submitted query, selection, group state and view scroll survive
hiding/reopening and Open–Back; preview intent is retired when hidden.

Query edits, pending or failed reruns, and reconnect make retained historical
rows inactive until another explicit successful search. An outstanding physical
search request disables duplicate submission even after its intent is retired.
Preview keeps the shared one-Read-in-flight boundary and 1 MiB limit; an open
draft previews with zero backend Reads. Opening reuses existing dirty tabs and
ordinary location history without implicit Save. Wide results/preview layouts
become stacked in narrower views, with spacing changes local to Find. Cancelling
or hiding a preview retires its intent: explicitly previewing the same target
again creates a fresh intent that cannot use the cancelled request's reply. The
existing request and preview limits are unchanged.

A very short explicit tools-height preference can clip or hide Find rows. Fully
invisible rows cannot activate through pointer or keyboard; **Automatic height**
is the recovery control. Targeted frame checks passed the ordinary and narrow
Find journeys with 13- and 18-point text. They do not establish usability of a
150-point tools preference with 18-point text, or of every viewport. Native GUI
and the complete aggregate remain separate gates. A retained scroll offset can
be clamped by the actual available scroll range after a layout change.

This is a retained-results subset of B2, not completion of the whole roadmap
phase. There are no new text filters, case/word options, regular expressions,
CamelCase matching, debounce, disk persistence, global-theme changes, protocol
changes or Java/JDT features. Existing filename discovery and its extension
contract remain unchanged. The user-facing contract is in
[Find and return](FIND_AND_RETURN.md).

### 0.61.0 local checks and then-remaining gates

The integrated local Linux **workspace, all-target, all-feature, locked** aggregate
passed **49 suites, 1,943 passed / 0 failed / 69 ignored** against the versioned
0.61.0 candidate. Formatting and strict workspace checks passed. The selected
Python checks recorded **207 tests: 206 passed, zero failed and one skipped**. The earlier pre-version
aggregate had the same Rust totals; it is not substituted for this final run.

The local Browse frame suite passed **44 tests**, comprising the retained 27
cases and **17 new retained-result cases**. The **33 Search tests** also passed
after adapting their expected controls to the current labels. These headless
egui checks cover grouped duplicate identities, visible-hit navigation, shared
snapshot retention, query round trips, pending and failed reruns, reconnect,
mixed/held/composition input, busy submission, hidden-row protection, safe height
recovery, retained/clamped scrolling, preview cancellation and request-identity
exhaustion. Standard and narrow journeys include 13- and 18-point text. Test
names using "native" refer to real egui input handling, not an OS-window run.

Both normal-agent proofs passed locally with the matching **0.61.0 agent**,
and both receipts passed strict validation. Earlier pre-version results are not
substituted for these final executions:

- Browse passed the strict **17-field / seven-operation** contract. Its six added outcomes
  verify the retained text snapshot, grouped original indices, exact text
  Open–Back, reopened selection, zero additional operations for grouped
  navigation, and zero additional Reads for retained dirty-draft preview.
- Search keeps **14 cases / 57 fields / 12 operations** through the normal
  agent/client path.

The complete outer CI driver was **not run locally**. These local executions do
not establish Windows execution, exact release-package audits or native GUI
acceptance. At this preparation checkpoint, public
exact-commit CI was pending, and no 0.61.0 native run had occurred.

### Preserved local failures and layout boundary

Earlier focused checks failed, and their outcomes remain part of the record:

- Result allocation left the End-selected row outside its actual clip even with
  13-point text. Allocation handling was corrected.
- A 150-point tools-height preference with 18-point text hid the first hit. This
  remains an explicit usability limit. An initial Automatic height recovery
  attempt also failed; Find-local automatic sizing now restores the journey.
  Safe clipping and recovery do not turn the original fixed-height failure into
  a usability pass.
- Two fixtures asserted egui presentation too early: the rendered enabled state
  immediately after model retirement, and repeated Shift+Tab before deferred
  focus movement settled. Corrections preserve immediate authority checks and
  exact recovery assertions using real input frames.
- An exact retained-scroll assertion failed when the viewport grew by 24 points.
  The resulting 24-point offset reduction was the exact maximum-scroll clamp;
  the final check uses observed content/viewport geometry without a tolerance.

Source review found that selecting A, then B, then A again could recreate an old
preview identity. A fresh serial now distinguishes each new preview intent,
including cancellation and reopening; an old reply cannot satisfy a later intent
for the same hit. Controlled headless tests cover this source-derived fix and
serial exhaustion. It is not a diagnosis of a native-window failure.

Passing regressions also cover deferred preview reveal yielding to newer
focus/scroll actions and fully clipped focused rows. These review-found risks
are not presented as earlier failed test executions. A separate local monitoring/observation failure is an
observation failure, not a failed product test or a substitute for its terminal
result.

### 0.61.0 native gate at preparation

The 0.61.0 native gate required independent exercise of Project Text result Open–Back,
reopening the retained query/selection/group state, and the keyboard Back/Forward
aliases against the exact candidate package. The 0.60.0 native omissions below
cannot be filled by headless tests or inherited as new native evidence. The
ordinary/narrow Find journey and the insufficient-height recovery boundary must
remain distinct; safe clipping with Automatic height recovery is not a general
150-point/18-point usability pass.

## Accepted 0.60.0 bounded browsing baseline

Public commit
[`bd5cf9fc38635545d94941044f18d9ee436df2ea`](https://github.com/LLLLimbo/cedar-ide/commit/bd5cf9fc38635545d94941044f18d9ee436df2ea)
passed [CI 38206608526](https://github.com/LLLLimbo/cedar-ide/actions/runs/38206608526).
All **four jobs** and **three exact-source packages** passed verification. The
complete workspace aggregates recorded **1,926 passed / 0 failed / 69 ignored**
on Linux and **1,866 / 0 / 113** on Windows, each across **49 suites**.
Both platforms passed all **27 Browse frame tests** and the normal-agent browsing
proof. Existing required runtime gates and independent real-JDT producer proofs
also passed; they are separate from native GUI evidence.

The browsing proof used exactly **seven operations**: Hello, List, an origin
Read, FindFiles, separate unopened-file preview and Open Reads, and Search.
Preview and opening of an already dirty draft added **zero Reads**. The proof
verified unchanged fixture sources and owned-agent cleanup. These are headless
normal-agent results, not native-window operation counts.

The bounded Linux native run used the exact CI desktop package with trust off.
It exercised file discovery, preview, ordinary Open, toolbar Back, dirty-draft
Undo/Redo, scoped discovery and Unicode-path display/open, plus Project Text
submission and Current open draft preview. The application exited successfully.
This accepts the stated 0.60.0 browsing milestone; it does not mean every roadmap
journey passed natively.

The native run **did not exercise** Project Text result Open–Back, reopened
Project Text query retention, keyboard Back/Forward aliases, Forward, native
Unicode character input, old-peer or incomplete-scan stress, Windows GUI, or real
SSH. Unicode paths were found using an ASCII query. It did not establish native
backend operation counts, exact dirty-buffer bytes or raw selection affinities
from screenshots, native agent reaping, peak memory, or comparative performance.
The new B2 retention and grouped-result interactions require their own evidence.

## Preserved preparation and historical reports

The original 0.60.0 preparation record follows unchanged. Its pending statements
and its then-current 0.59.3 baseline describe the period before the accepted
0.60.0 CI/package/native evidence above. Earlier failed checks and native limits
remain part of the record; none are relabelled as passes. This historical text
does not report execution or acceptance of the 0.61.0 candidate.

## 0.60.0 scope and acceptance state

**0.60.0 is an integrated browsing candidate, not an accepted release. Full
exact-commit dual-platform CI, all three exact-source release-package audits and
native-window acceptance remain pending.** Local focused checks establish only
their stated scope. Historical acceptance below does not transfer to these new
production paths, and no new Windows GUI or real-SSH acceptance is claimed.

The candidate combines the shared **Find and return** flow with bounded filename
discovery into untouched workspace directories. **Files** and **Project Text**
use explicit submission, immutable result activation, explicit read-only preview,
ordinary Open and existing location history. Open drafts are previewed without a
backend Read; dirty text, Undo and save state are preserved. Filename opening
retains an existing draft's selection, while a text hit uses its historical line
hint. These actions do not implicitly Save. The complete user contract is in
[Find and return](FIND_AND_RETURN.md).

Project files requires the supported optional Hello `file_discovery` version-1
descriptor. Protocol 4, AgentInfo schema 1, the 32 direct-capability limit and
two-group limit remain unchanged. Absent or unsupported extensions retain the
explicit **Open and loaded files** fallback and send no discovery requests.
Discovery reads metadata only, has a visible directory scope and exclusion
policy, reports bounded incomplete reasons, and does not create an index or
watcher. Literal filename/path matching uses Rust Unicode lowercase without
normalization or full case folding; regex, CamelCase and live search-as-you-type
are not implemented. The elapsed deadline is cooperative, not a guarantee that an
individual blocking filesystem operation can be interrupted.

Existing Build/Run and language tools are retained under their existing support
and trust requirements. No new execution trust, automatic language startup, SSH
deployment or remote transport is introduced. There is no visual-parity or
unmeasured performance claim. The browsing roadmap remains the design decision;
implementation and acceptance status are reported here separately.

## 0.60.0 local documentation and package checks

The user guide is linked from both READMEs and all three quickstarts, and is an
explicit required payload in the Windows development, Linux agent and Linux
desktop package inventories. Focused Python package/link regressions passed on
Linux:

| Family | Result |
| --- | --- |
| Windows bundle | 37 selected: 36 passed, one Windows-only junction check skipped, zero failed |
| Linux agent bundle | 31 passed, zero failed |
| Linux desktop bundle | 40 passed, zero failed |

These tests use synthetic binaries and the real guide text. They do not build or
validate current shipping binaries, substitute for the three release-package
audits, prove native GUI behavior, or establish real SSH interoperability. The
Rust/process results below come from their separate terminal runs; they are not
inferred from these documentation checks.

The local Linux **workspace, all-target, all-feature, locked** aggregate passed:
**49 suites, 1,926 passed / 0 failed / 69 ignored**. All **26 retained Python
test suites** also passed. The new browsing acceptance runner passed its **10
pure tests**, and its strict validator accepted both actual normal-agent proof
outputs. These are local results; exact-commit dual-platform CI, Windows
execution, current release-package audits and native-window acceptance remain
pending.

## 0.60.0 focused frontend verification and preserved failures

Final formatting and strict workspace, all-target, all-feature Clippy checks
passed on the final Rust sources, including the fixture-only baseline corrections.
Focused checks passed the additional Ctrl+Alt+Left/Right history aliases
and Alt+1 Project access, including positive activation, mixed-input rejection
and held-key behavior. Queued-Open and residual-input ownership checks also
passed. The aliases retain the existing bracket history shortcuts and
Ctrl/Cmd+Shift+E. The final focused run recorded **27 passed / 0 failed / 1 ignored**.
The ignored normal-agent proof was then executed explicitly using the final
0.60.0 agent and app; its exact seven-operation journey, source hashes and close
checks passed. The full local workspace aggregate passed separately as recorded
above. Neither result establishes native GUI acceptance.

An integrated focused run recorded **26 passed / 1 failed / 1 ignored**. The
failure was
`browse_tests::browse_global_entry_rejects_repeat_held_and_ime_input_without_stealing_row_activation`.
The fixture incorrectly assumed an empty native egui Preedit event was inert
while an editor selection was active; native editing deletes that selected
range. The correction collapses the selection with native ArrowRight only for
the IME case and checks the exact preconditions before the measured event.
A subsequent IME fixture assertion also failed because native Preedit changes
caret affinity. Both failures concern expected native editing behavior in the
fixture; production behavior and input guards are unchanged by the corrections.
The final focused rerun passed as recorded above; both earlier failures remain
preserved and are not reclassified as passes.

## Accepted 0.59.3 baseline and preserved pre-CI record

### Final accepted 0.59.3 evidence, verified 2026-10-12

**0.59.3 was the accepted bounded checkpoint at this stage.** Public commit
[`1e2a26d6dd38c17ed3723ced54797fcb43255eb6`](https://github.com/LLLLimbo/cedar-ide/commit/1e2a26d6dd38c17ed3723ced54797fcb43255eb6),
tree `d3f951aa8acd77efebad5aa42c8d773563016c5e`, passed
[CI 38192432380](https://github.com/LLLLimbo/cedar-ide/actions/runs/38192432380).
All four jobs and all three exact-source packages passed. Linux recorded
**1,862 passed / 0 failed / 67 ignored** and Windows recorded
**1,805 / 0 / 111**, each across 48 suites. Recovered public logs, package manifests
and receipts were independently rechecked on 2026-10-12; this is a recovery audit
of that original run, not a new CI execution.

Its native acceptance carries the 0.59.2 functional evidence and separate cleanup
complement through verified shipping-source equivalence. The original native
screenshots and receipts were unavailable for this audit, so their contents were
not reverified. Historical acceptance and the original 125.452-second cleanup
failure remain intact; there is no fresh native validation or Windows GUI claim.
The same-package 28.175-second complement retains its original bounded meaning.

The historical status summarized below describes the 0.59.3 source submitted on
2026-10-12 before its CI run completed. Pending statements are historical and
superseded by the accepted evidence above. The 0.59.3-only production-equivalence
and unchanged-inventory statements do not describe the new 0.60.0 implementation.

### Original scope and acceptance state

**0.59.2 is the latest fully accepted bounded checkpoint. 0.59.3 is a test-only, CI-only candidate with exact-commit CI and package verification pending.** This checkpoint adds deterministic Unicode Parameter info checks against the actual egui renderer. Production Rust and Python behavior, the workflow, dependencies, protocol/capabilities, parser limits, receipt predicates, operation inventories and all test/proof budgets remain unchanged. The workspace, Cedar lockfile records and retained exact-version assertions advance to 0.59.3.

No new native GUI run is part of 0.59.3. Its native provenance is carried from the exact accepted 0.59.2 package through a production-source-equivalence audit, with the original native limits retained. That audit does not turn authored renderer tests into executed tests, prove a new platform, or establish broad Unicode/IME behavior. The new Rust tests remain unexecuted locally and require the fresh dual-platform CI aggregate. The command-output banner remains backlog.

## Final accepted 0.59.2 evidence

The accepted public source is [`a3f69b469f16b8bd66b2f57e38aaae2bab759281`](https://github.com/LLLLimbo/cedar-ide/commit/a3f69b469f16b8bd66b2f57e38aaae2bab759281), tree `b20d684046c1bcf86c07a8db6420133b1af590af`, tested by [CI 38183040039](https://github.com/LLLLimbo/cedar-ide/actions/runs/38183040039). **All four CI jobs and all three source-linked packages passed independent verification.** Both strict checks, complete workspace aggregates and retained process/runtime gates passed. Linux recorded **1,861 passed / 0 failed / 67 ignored**, Windows **1,804 / 0 / 111**, each across 48 suites. Each platform passed the 24 Java-signature Python driver tests and the deterministic Parameter info groups: 16 frontend, 12 pure proof, six parser, five backend and two client tests.

Both independent real-JDT producers used the normal agent/normal client route and the same-run, exact-source packaged agent. Each issued exactly three Parameter info queries: first-argument and second-argument results contained **2 / 2** signatures with the exact expected active parameter, and the outside-call query returned **0**. Their exact ledgers, synchronized source, server provider support, unchanged Java source and owned cleanup were verified by the bounded headless receipts. These receipts explicitly do not prove native GUI behavior.

The three accepted archive hashes are Windows development `157836d03ba29c1d0d7915a2aa64da7bba497ad92cd07c366066c8c3c86294d1`, Linux agent `5001ef7d4efb4f0e9c994677c5588a3a5270b497549b8457b3bed1fe32d79809`, and Linux desktop `a2cfe3d921030651a34ca160fdef4c4324e24a745af5b1dd275b6658126598d3`. The Linux desktop binary is `f1e308cfffe05d67fadf88debfb3ba0809a8abc0b5a7b20c9e160871145cbe58`; its companion agent is `c4ed462427c6aff21b5ecdf0c00b45e25ca1ed5050fe7ef429078bf92697d0f3`. These are 0.59.2 artifacts, not newly built 0.59.3 binaries.

### Bounded Linux native evidence and retained first failure

The first exact-package cloud Linux native run observed both active arguments, both overload labels, an empty outside-call result, cursor/edit invalidation and explicit dismissal. An inserted `8` remained visibly selected in dirty `78` before the query and after 64 native Tab presses back to the editor, with no pointer refocus after dismissal. Undo was the next native input and restored clean `7`; the next Redo restored dirty `78` and selected `8`. Some grouped Tab sequences have only terminal screenshots; no invisible focus owner is inferred. The run observed Java graceful-exit text and frontend exit 0.

That run's strict cleanup gate **failed**: the recorded observation lasted **125.452 seconds**, exceeding the original **120-second** bound by 5.452 seconds. This failure remains failed. A separate same-package cleanup-only complement observed a dirty draft, one correct second-parameter query, normal Stop, explicit Discard/quit, frontend exit 0 and exact-window absence in **28.175 seconds**, within its separate unchanged 120-second bound. The complement supplements the acceptance scope; it does not reclassify the first attempt, repeat the functional matrix or supply new Windows GUI evidence. The complement did not open Java's graceful-exit detail before quitting, so no Java exit-code claim is made for that complement.

Both generated native workspaces retained the original 322-byte Java source, SHA-256 `2a196b0dc072953dcf36761bb91532ccbc6c64539b2d1cdcdb4857ab33045211`. JDT itself rewrote generated `.project` metadata from 282 to 675 bytes and added a 737-byte `bin/CedarParameterFixture.class`. Thus **whole-workspace equality was not met** and is not claimed; no Save or Run action explains away those explicit background-generated changes.

Acceptance combines the first functional observations and the separately verified cleanup complement. It does not establish native backend request counts, agent reaping, delayed/in-flight response races, Windows GUI, broad IME/Unicode input, restart/persistence or broad workspace stress. The headless proofs independently establish their own exact ledgers and cleanup only.

The [original 0.59.2 preparation report](TEST_REPORT_PHASE592_CANONICAL_FIXTURE.md) is archived **byte-for-byte**, 30,437 bytes, SHA-256 `957ae8fd643fa3abb161fec1ef24a1d6be24a04900e16d9a39b17ee322754ca3`. Its CI-first PARTIAL and native DEFERRED wording records the earlier preparation state, not the terminal result above. The 0.59.0 and 0.59.1 failed runs remain preserved below and in their unchanged archives. Passing 0.59.2 does not retrospectively prove that a Windows temporary-path alias caused the 0.59.1 failures; that explanation remains a source-supported, runtime-unconfirmed candidate.

## Narrow 0.59.3 renderer-test scope

The only behavioral source edit is in the test-only `crates/app/src/java_parameter_info_tests.rs` module. The existing `overload_browsing_keeps_selection_history_draft_disk_and_undo_unchanged` test is retained and strengthened with three overloads, a Unicode label containing `🦀`, `e` plus a combining mark, a literal UTF-16 parameter interval `[17, 28]` and independently checked UTF-8 byte interval `20..34`. It inspects the actual renderer's `LayoutJob` text and ordinary/emphasized sections, clicks real overload controls and returns to the first row, and preserves draft/disk bytes, full selection affinities and Undo/Redo history.

One new test, `native_signature_rendering_does_not_emphasize_missing_invalid_or_ambiguous_hints`, covers four renderer cases: no active hint, an out-of-range global hint, an out-of-range per-signature hint overriding a valid global hint, and an ambiguous repeated string label. The Parameter info frontend module now contains **17 tests**, previously 16. These are headless egui renderer checks despite the test's `native_signature` name; they are not a new native GUI run. Both tests remain **authored and unexecuted locally**, pending exact-commit CI; source inspection alone cannot establish a passing render.

Retained Duplicate, Search and report acceptance drivers, Python tests and Rust proofs change only fixed product-version literals. The existing Java-signature Python driver's synthetic version fixture also advances by literal substitution only. No production rendering code, shared parser, backend/client implementation, acceptance predicate, budget, deadline, retry, CI workflow or package inventory is changed. The existing 0.59.2 native provenance is inherited through this bounded production-equivalence claim; no 0.59.3 GUI launch is claimed or scheduled.

## Final accepted 0.58.4 evidence

The accepted source is public [`9e925cc4dc084b3893bf7ed0cc907990bb22047b`](https://github.com/LLLLimbo/cedar-ide/commit/9e925cc4dc084b3893bf7ed0cc907990bb22047b), tree `f6d4a32e4db06f192c95071065d08a1d873f1ac9`, tested by [CI 38174023016](https://github.com/LLLLimbo/cedar-ide/actions/runs/38174023016). Both strict checks, complete workspace aggregates, retained process gates and all three package audits passed. Linux recorded 1,818 passed / 0 failed / 66 ignored; Windows recorded 1,761 / 0 / 110; each completed 48 suites. Each platform passed 15 editor-entry, 27 resize, 23 sidebar, 33 Search and 19 report-keyboard tests. The normal-agent report proof matched 15 cases / 55 strict typed fields / 10 requests / two owned agents reaped. Search retained 14 cases / 57 fields / 12 requests / two owners reaped.

The exact-package fresh cloud Linux GUI run observed genuine divider dragging, pointer reset, report load/clear in automatic mode, a later genuine drag, and keyboard focus followed by Space reset. From the disabled reset onward, 37 real forward Tab presses were captured separately. The critical editor-entry witness is step **56**, followed by immediate Undo at **57** and Redo at **58**, with no intervening pointer refocus. Dirty four-line text and full selected block survived Tab; Undo restored the original clean two-line text and full selection; Redo restored the dirty duplicate and selection. Step 55's anticipatory filename is not the editor-entry witness. Several earlier focus stops lacked visible highlights and have no inferred owner. No Save, Run or LSP action was used, and source/fixture disk bytes remained unchanged.

This is bounded visible evidence for one ASCII CRLF draft, one generated report and one fresh cloud instance. It does not establish Windows GUI, raw event/frame ownership, backend operation counts or native agent reaping, broad IME/Unicode behavior, OS resize or Tools restart coverage. The pinned-egui immediate-next-Tab lock-registration limitation and lack of a cross-frame held-key lineage guarantee remain.

The [original 0.58.4 candidate report](TEST_REPORT_PHASE584_TAB_BOUNDARY_FIXTURE.md) is archived unchanged: 20,338 bytes, SHA-256 `966b95ef62c5c505837b2302d3abf33272eef2f4aa39f58dfaf5638afdb59340`. Its pending statements describe the earlier preparation checkpoint, not the terminal result above. The archive retains the 0.58.3 eight-case boundary-focus fixture failure, the 0.58.2 measured Tab mutation, all earlier failed reports, and the exact distinction between test preparation and shipping behavior. No prior failed run or native limit is erased.

## Preserved 0.59.0 first CI outcome

The tested public source is [`653c75063394677f8e8aceca3f73fcf24acf51b4`](https://github.com/LLLLimbo/cedar-ide/commit/653c75063394677f8e8aceca3f73fcf24acf51b4), tested by [CI 38180274662](https://github.com/LLLLimbo/cedar-ide/actions/runs/38180274662). The [original 0.59.0 preparation report](TEST_REPORT_PHASE59_PARAMETER_INFO.md) is archived byte-for-byte: 14,593 bytes, SHA-256 `05dafba387dd5209e607dfc5d31dfe47b7b79162ea068184f0fea4cf3b64ad72`. Its pending statements describe preparation before that CI attempt; they are not a terminal CI result or evidence for 0.59.1.

Windows selected 19 tests in `scripts/test_java_signature_acceptance.py`; five failed before the Windows strict Rust compilation, complete workspace aggregate, runtime gates and package production. The raw failures were:

- `test_short_python_child_success_nonzero_flood_and_timeout`: the real tiny Python child's output was `b"ready\r\n"`, while the test asserted `b"ready\n"`.
- `test_native_schema_and_static_fixture_digest_match_the_driver`: the decoded source fixture hashed to `3cfb97f63ad83bbcc66b9a9b4b3cc657155238337f63c3f9194d82e2f76ff91b`, rather than the required `2a196b0dc072953dcf36761bb91532ccbc6c64539b2d1cdcdb4857ab33045211`.
- `test_both_platforms_exact_stage_budgets_and_no_offline_or_bundled_workflow`: the mocked driver returned failed status rather than passed.
- `test_every_failed_stage_is_terminal_and_original_failure_preserved`: the mocked runner recorded zero calls rather than the expected first call.
- `test_failed_native_record_is_retained_and_changed_source_cannot_pass`: the expected probe record was absent.

The last three raw assertions did not log the driver's failing stage. A source-only reproduction decoded the same Rust fixture bytes with explicit UTF-8 and cp1252: UTF-8 reconstructs 322 bytes and the required `2a196b...45211` hash; cp1252 reconstructs 331 bytes and the observed `3cfb97...ff91b` hash. The old Java release parser accepts an LF fixture and rejects the corresponding CRLF fixture. The mocked release file used platform-translating `write_text`, and the parser runs before the first mocked child call. A Windows CRLF fixture causing those three downstream assertion failures is therefore a source-supported inference, not a directly logged runtime stage.

Linux strict checks, all-feature release compilation and the complete aggregate passed in the same 0.59.0 run: **1,861 passed / 0 failed / 67 ignored across 48 suites**. The new deterministic coverage passed: 16 frontend, 12 pure proof, six parser, five backend and two client tests. These passes do not execute the ignored three-query real-JDT Parameter info proof.

The Linux job then failed in **Agent/tool protocol chains** at `scripts/capability_smoke.py:74`: the exact symmetric difference was `{'language_java_signature_help'}`. The preceding stdio protocol smoke passed. Both the retained Python `TYPED_JAVA` inventory and its independent test inventory still described 31 direct capabilities, omitting the 32nd operation already advertised by the Rust implementation. This is an acceptance-inventory omission; no new Rust capability is introduced by the 0.59.1 correction. Later Linux runtime/package gates were skipped. Windows also never reached its package gates.

The separate real-JDT Parameter info producer depends on the whole `test` job matrix and was **skipped** after those prerequisite failures; neither platform producer ran. The completed run produced **zero artifacts**: no Windows development, Linux agent or Linux desktop package was produced or accepted, and no real-JDT Parameter info receipt exists for this run. The workflow's failure, Windows's five Python assertions, the Linux smoke failure and all skipped gates remain failures/skips; no unchanged retry or replacement result is substituted.

## Preserved 0.59.1 terminal CI outcome

The tested public source is [`ecdb06ead210f5c47b99dd47ccef661dcd7d7248`](https://github.com/LLLLimbo/cedar-ide/commit/ecdb06ead210f5c47b99dd47ccef661dcd7d7248), tested by [CI 38181570382](https://github.com/LLLLimbo/cedar-ide/actions/runs/38181570382). The [original 0.59.1 preparation report](TEST_REPORT_PHASE591_WINDOWS_TEXT_BOUNDARIES.md) is archived byte-for-byte: 22,570 bytes, SHA-256 `aaf0d62b6e786f440f2c762318981630571c682af51cbedc710e90ea045db4a4`. Its pending statements describe preparation before that CI attempt; its local 298-test pass does not establish Windows-host success.

The Windows Python gate selected 22 tests and recorded **six failure records**. Four belong to the single success-path method's platform × release-ending subcases; they are not four additional selected tests:

- `test_both_platforms_exact_stage_budgets_and_no_offline_or_bundled_workflow`: all four mocked `linux`/`windows` × LF/CRLF subcases returned failed status, logging `compile/driver_error`.
- `test_every_failed_stage_is_terminal_and_original_failure_preserved`: the next-stage fixture recorded five runner calls rather than the expected six, so it did not reach its intended selection-stage failure.
- `test_failed_native_record_is_retained_and_changed_source_cannot_pass`: the expected partial probe record was absent; the fixture did not reach the intended runtime receipt write.

This advances beyond the 0.59.0 failures at UTF-8 source decoding, CRLF release parsing and deliberate tiny-child byte output: their dedicated tests no longer failed in the 0.59.1 Windows run. It does not establish success for the remaining driver fixture. The six records do not print the underlying validator exception or either path's spelling. The original `compile/driver_error`, five-versus-six call mismatch and absent probe remain failed evidence.

Source inspection identifies a specific mismatch candidate: `DriverTests.exercise` originally constructs its owned base with `Path(temporary)`, then puts that spelling in synthetic Cargo metadata. The retained `compiled_test` validator requires `executable.resolve(strict=True) == executable`. A noncanonical temporary-root alias could therefore fail at this exact compile boundary before selection. The real driver's entry point already derives its checkout root from `Path(__file__).resolve().parent.parent`. This is a source-supported fixture-alias candidate, not a logged Windows-path diagnosis or a production-root correction. The fixture must meet the existing strict validator contract; the validator is not relaxed.

The terminal Linux job passed strict checks, the complete aggregate (**1,861 passed / 0 failed / 67 ignored across 48 suites**), release compilation and all retained runtime/process gates. Both the Linux agent and Linux desktop packages were produced and independently verified against this exact 0.59.1 source. These are retained-gate and partial package results for 0.59.1; they do not establish the new independent Parameter info proof, a Windows package or 0.59.2 acceptance.

The Windows failure blocked its strict Rust compilation, complete workspace aggregate, runtime gates and package production. The separate real-JDT Parameter info producer depends on the whole `test` matrix, so **both platform producer proofs were skipped**. No Windows package or real-JDT Parameter info receipt exists for this run. The workflow remains failed despite the successful Linux job and its two verified packages. The six Windows failure records, skipped producer proofs and missing Windows package remain preserved; the Linux evidence cannot substitute for them or for positive native acceptance.

## Explicit Java request and compatibility

The optional `language_java_signature_help` operation is the 32nd direct capability on the supported typed-Java isolated-agent routes. Protocol 4, capability schema 1 and the 32-name limit remain unchanged; existing Maven capability groups are retained. Older agents can omit this capability and continue serving the retained operations. Capabilities do not confer execution trust.

A request requires execution trust, a running production typed Java session, a workspace-contained `.java` document synchronized as Java, a positive exact acknowledged document version, the new backend capability, and actual server signature-help support. Generic LSP or a validation host cannot become production Java merely by claiming JDT identity or provider support. Server support accepts a provider object, with boolean `true` accepted for compatibility; no arbitrary server callback is added. Dispatch uses the captured UTF-16 cursor and exact synchronized version. Unavailable or stale identity must not send a speculative query.

Both basic-Java and Maven initialization builders set `java.signatureHelp.enabled` to true and disable `java.signatureHelp.description.enabled`. This controls the explicit server feature; it does not turn typing into a query or start a server. Existing synchronization, import and trust boundaries remain. Server-side diagnostics and ordinary startup behavior are not new Parameter info callbacks.

## Bounded plain-text results

The agent and desktop share one bounded parser: at most 32 signatures, 64 parameters per signature, 8 KiB of UTF-8 per retained label, and 128 KiB of total retained UTF-8 label text. Null and an empty signature array mean no result. Malformed shape, invalid label/offset types, invalid UTF-16 boundaries and exceeded limits reject the complete result. UTF-16 parameter offsets must be ordered and within the signature label; surrogate-pair splits are rejected. A zero-length range yields no visible highlight. A string parameter label must be nonempty and occur in the signature; only one occurrence, including overlapping matches, permits a highlight. Ambiguous matching produces no guessed range. Documentation and extension fields are not retained or interpreted; displayed text is inert.

A valid server `activeSignature` chooses the initial row. Otherwise the UI can display row zero as a local fallback, clearly distinct from a server-selected overload. A signature's own `activeParameter` takes precedence when present; an in-range top-level hint is used only for the initially displayed signature when that signature has no own hint. A present but out-of-range per-signature hint yields no active parameter and cannot fall back to the top-level hint. Missing or unusable hints do not invent parameter zero. This deliberately differs from LSP 3.17's default-to-zero selection rules and is documented in the guide. Browsing overloads is a local display action, not another query or a claim that the browsed overload is the server choice.

The frontend captures the source document, edit version, cursor, workspace/connection and language session. Changed identity invalidates a pending or displayed result. Navigation, newer queries, hiding the view, explicit Dismiss parameter info and session changes cannot revive an old response. Requesting or browsing help must preserve draft bytes, selection, Undo/Redo, saved baseline and disk files. Its status remains a historical server answer for the captured source position; synchronization does not establish index freshness or compiler correctness.

Ordinary pre-query synchronization retains its existing job and global-error semantics. Dismissal cancels the Parameter info intent and rendering, while an already submitted synchronization can still finish or report a failure through the existing language status behavior. Existing synchronization identity checks remain unchanged; this checkpoint does not claim stronger filtering of ordinary global error messages. That outcome cannot revive the dismissed panel, send its cancelled query or apply a stale Parameter info result. No tagged synchronization job, new retry or synchronization suppression mode is introduced.

## Unchanged independent CI proof contract

The retained Parameter info proof has its own CI job and bounded driver. It does not replace or loosen any existing gate, its deadline, assertion or receipt schema. The job has a 35-minute limit. The driver budget is 28 minutes: compile 600 seconds, Java preparation 180, checks 35, test selection 15 and runtime 780, totaling 1,610 seconds with 70 seconds reserved for failure teardown (20), hashing/final receipt (30) and orchestration (20). The 35-minute job allocates 60 seconds each to checkout, Python setup, Rust setup, download, package verification and receipt upload, plus the 1,680-second driver, totaling 2,040 seconds. Exact test selection and success receipts are required; merely finding a test executable or returning a process status cannot establish the proof.

The Rust proof reserves 570 seconds for active work and 180 seconds for cleanup inside the driver's runtime bound. Its fixed ordinary setup inventory is Hello 1, List 1, Read 1 and Java Begin 1. Startup polling is bounded by 1,024 polls, a 50 ms interval and one nonrenewing 75-second startup window; setup controls are bounded to 30 seconds each. The intended positive inventory is Open 1, Parameter info queries 3 and Close 1, each with a 75-second bound. Stop and owned-process reaping must finish within the separate cleanup bounds; a failure in active work cannot suppress cleanup or produce success. This exact proof contract passed for 0.59.2 as recorded above; fresh 0.59.3 receipts remain pending.

Required fresh evidence includes exact-commit strict checks and complete workspace aggregates on both platforms, all retained normal-agent/process gates, the unchanged real-JDT Parameter info proof, and source-linked Windows development / Linux agent / Linux desktop package audits. Retained deterministic parser, backend and UI coverage must continue to establish capability/trust/version gating, malformed and excessive payload rejection, UTF-16 and ambiguous-label handling, conservative active hints, stale replies and local dismissal. The bounded 0.59.2 Linux native evidence is separate from these headless gates; 0.59.3 adds no new native run.

## Source/version, package and local verification boundaries

The workspace and exactly ten Cedar lockfile records move from 0.59.2 to 0.59.3. All 343 non-Cedar package records, every dependency edge and the 11 other crate/example manifests remain unchanged. Cargo dependency resolution is not run. Nine retained Duplicate/Search/report driver, test and proof files plus the Java-signature synthetic test fixture change only exact version literals. The three package source inventories and existing entry-guide link order remain unchanged: Windows 23 links, Linux agent five and Linux desktop 14. No JDK, JDT, new binary or third-party dependency is bundled.

This preparation used Python/static checks and formatter-only inspection. It included **no local Rust compile/runtime, JDT startup or new native GUI run**. Those checks did not establish CI, package or native acceptance.

## 0.59.3 local verification and remaining acceptance

The recorded local Python run selected **321 tests: 320 passed, one expected Windows-only skip, zero failures**, in **5.887 seconds**. It covers the same 13 suite families as the 0.59.2 preparation, including all 24 Java-signature driver tests and retained/package suites. These are Python/static checks using synthetic payloads and mocked Rust/JDT paths; they do not execute the new Rust renderer tests or establish Windows-host, real-JDT or native behavior. The formatter-only workspace check, AST parsing of all 64 Python scripts and diff-whitespace check passed. No local Rust compile/test/runtime was used.

The recorded source-equivalence review identified only the renderer test change and product-version updates; production behavior, workflows, validators, budgets and dependencies were unchanged. At this preparation stage, the renderer tests still required exact-commit CI execution. Acceptance required strict checks, full workspace aggregates, retained runtime gates, real-JDT producer proofs and all three exact-source package audits. Carrying 0.59.2 native evidence required verified production equivalence and retained its first cleanup failure and all original limits. These preparation checks alone did not establish 0.59.3 CI, package or fresh native acceptance.

The byte-identical 0.59.2 archive retains all earlier local verification results, including its 321 selected / 320 passed / one expected skip Python run, the source/version audits and the original CI/native pending state. Those historical local checks do not substitute for new 0.59.3 execution.
