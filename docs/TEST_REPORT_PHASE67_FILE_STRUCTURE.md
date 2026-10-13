# 0.67 File Structure — CI-first candidate

**0.67 is a CI-first candidate, not yet accepted.** Final local checks passed:
**17 actual-frame tests**, Rust **2,145/0/71 across 50 suites**, Python **803 total
/ zero failures / ten skipped across 29 suites**, independent source review,
strict Clippy and formatting. The selected-artifact Linux normal-agent/mock
proof passed **45 fields / 11 operations**; retained Browse and Scoped Search
proofs also passed. The two earlier failed runtime attempts remain preserved
below. Exact CI and three package audits remain pending. No new positive native
JDT or GUI Trust-enabled validation was run; local results are partial evidence,
not full native acceptance or IDEA parity.

## Bounded feature contract

File Structure presents the existing exact outline snapshot in a dedicated
current-file popup. The slice provides **Ctrl+F12** on Windows/Linux and
**Cmd+F12** on macOS, a case-sensitive literal symbol-name filter bounded to
**256 Unicode scalar values**, source order, kind and hierarchy display, and exact
Enter/click declaration navigation followed by ordinary Back. It preserves
dirty text and Undo/Redo. Its usage and limitations are in
[File Structure](FILE_STRUCTURE.md).

Only an already running, matching provider with document-symbol support is
eligible. A current exact cached outline is reused; explicit invocation or
Refresh follows the existing synchronization/documentSymbols path. Filter
edits send no requests. The popup’s request destination is independent of the
Language tool. Document edits, tab/session changes, Stop, Refresh and dismissal
invalidate obsolete rows and intent; late replies cannot revive the popup or
focus. Flat SymbolInformation remains unversioned and uses existing checked
agent URI resolution. No inherited members, CamelCase, parser, provider,
protocol, persistence or automatic trust/startup is added.

## Evidence required for this round

| Gate | Status and evidence boundary |
|---|---|
| Source review | Independent review passed for the narrow rapid-reopen correction. It changes admission from the popup’s own modal and clears obsolete editor-focus restoration, preserving foreign-modal refusal. |
| Focused and actual-frame checks | **17 local actual-frame tests passed after correction**, including immediate Escape/reopen/text input without settling frames, unchanged editor bytes, foreign-modal refusal and pending-activation completion. This is local frame evidence, not Windows or native GUI evidence. |
| Linux normal-agent/mock-provider proof | **Third runtime passed:** strict **45-field** receipt and exact **11 operations**, nine active plus two cleanup. The two earlier failed attempts remain separate below. |
| Windows new positive coverage | Windows frame execution awaits exact CI. No new positive Windows typed-Java provider journey is measured; the Linux mock proof cannot establish one. Generic Windows LSP remains unsupported. |
| Aggregate and strict checks | Final same-source Rust: **2,145 passed / zero failed / 71 ignored across 50 suites**. Python: **803 total tests, zero failures, ten skipped across 29 suites**, including all ten new proof-driver cases. Strict Clippy and formatting passed. |
| Retained selected-artifact proofs | Browse passed **50 fields / 14 operations**. Scoped Search passed **57 fields / 12 cases / ten operations**. These are local current-artifact results, separate from exact CI. |
| Exact dual-platform CI | Pending. Verify the final source revision, all required jobs and retained platform/JDT checks. |
| Three package audits | Pending. The new guide is in all three source inventories; current recursive packaged-guide closure checks passed separately, without a release build or exact-package audit. |
| Positive native File Structure / real JDT | Not run in this round. No native-positive acceptance follows from frame or mock results. |
| Trust-off native unavailable path | Not run. A later negative-path observation would not validate positive semantic navigation. |

## Final local verification

The final same-source Rust aggregate passed **2,145/0/71 across 50 suites**,
including all **17 File Structure actual-frame tests**. The normal-agent test
remains ignored in the aggregate; its explicit successful process run is
recorded separately below. Python passed **803 total tests / zero failures /
ten skipped across 29 suites**, including the final ten proof-driver tests.
Independent source/platform review, strict workspace/all-target/all-feature
Clippy with warnings denied and `cargo fmt --all --check` passed.

The final selected default-features app and matching normal agent passed retained
Browse **50 fields / 14 operations** and Scoped Search **57 fields / 12 cases /
ten operations**. An earlier Browse invocation was rejected by the local harness
before the test began; the subsequent admitted invocation passed. That local
admission failure is preserved separately from behavior checks. No source change
occurred after the final reviewed snapshot.

## Preserved local preparation and process history

The pre-runtime frontend passed **14 actual-frame tests**. The full same-source
Rust aggregate passed **2,142 tests / zero failures / 71 ignored across 50
suites**. It includes all 14 new frame cases; the added normal-agent proof is
ignored in that aggregate and has a separate runtime result below.
The retained Python aggregate passed **28 suites / 793 total tests / zero
failures / ten skipped**. The final new proof-driver suite passed **ten tests /
zero failures**, for a combined **29 suites / 803 total tests / zero failures /
ten skipped**. Its checks cover the recursive 45-field success receipt, exact
ledger, input and selection validation, unsupported/gated paths, bounded failure
receipts and a failed runtime’s diagnostic sidecar. Strict workspace/all-target/
all-feature Clippy with warnings denied and `cargo fmt --all --check` passed.
These passes preceded the failed runtime and production correction below.
They remain prior-candidate evidence; the corrected-source results are recorded
separately above.

The first mock-artifact build invocation omitted its existing required
`test-server` feature, so Cargo rejected it before compilation. That invocation
failure is retained; selecting the default-features normal agent separately from
the explicitly featured mock requires no source correction. Artifact selection
and the runtime proof are still separate gates.

### Preserved first normal-agent failure

The first actual run reached **six active operations**, then failed its
cached-filter assertion on an immediate **Escape → Ctrl+F12 → text input**
sequence. Both owner-cleanup operations succeeded, for an observed **eight
operations**, not the required full 11-operation success receipt. The failed
fixture is retained. Unit helpers had used three settling frames, which did not
cover this immediate sequence. The no-settle regression and negative control
confirmed a production defect; the process journey was not relaxed with extra
pacing.

The correction allows invocation from the popup’s own modal while preserving
foreign-modal refusal, and clears the obsolete editor-focus restoration when
reopening. The corrected focused set passes **16/16**, including immediate
Escape/reopen/text input with no settling frames, text reaching the filter,
unchanged editor bytes and the foreign-modal negative. Independent source review
passed. The third current-app runtime and final aggregate/strict revalidation
then passed as recorded separately.

### Preserved second runtime fixture failure

The second runtime passed the corrected immediate-reopen/filter sequence, then
failed a later exact-selection assertion. An actual-frame replica established
that the popup had closed while an accepted activation ticket was still pending;
production had requested the next frame. The helper treated modal dismissal
alone as completion and omitted that ticket from its witness.

The correction is limited to waiting for that existing activation to settle
within the helper’s **existing maximum of three frames**. It preserves the
exact **0..1 selection** assertion and zero additional operations. This is a
test-observation correction, separate from the first run’s production defect;
no further production behavior change or extra frame allowance is claimed. The
second failed run is retained; the third runtime passed with the corrected
completion witness. The final focused set includes this actual-frame regression
and passed **17/17**.

### Passed third normal-agent runtime

The third current-app run passed the strict **45-field** receipt with exactly
**11 Client operations**: Hello, List, Read, language Start, document Open,
document Symbols, document Change, document Symbols, document Symbols, document
Close and language Stop. That is **nine active operations plus two cleanup**,
with **three document-symbol requests and ten mock-LSP messages**.

The receipt verifies current snapshot/cache reuse, local filtering, exact
navigation, Back and normal Undo input without extra Client operations. The
cancelled request’s actual agent reply is retired without reopening the popup
or retargeting the Language tool. Fixture source bytes remain unchanged; Stop,
agent reaping and fixture removal passed. This is the ordinary Client/agent
route with a controlled mock provider, not a positive native or real-JDT popup
journey. The local run used the current selected default-features app, matching
normal agent and compiled mock artifacts. The canonical compile driver’s local
coverage is Python-mocked; that full compile path was not run locally, and exact
CI remains a separate pending gate.

### Normal-agent proof contract

The Linux proof uses the ordinary app/client/agent route and controlled existing
mock LSP. Its exact ledger is **nine active operations plus two cleanup
operations**. The fixture explicitly disables automatic background language
synchronization so the proof measures explicit synchronization, cache reuse,
refresh and navigation deterministically. This test setting does not change the
shipping automatic-sync behavior or claim coverage of automatic background
interleavings. The proof must validate its success fields and cleanup receipts;
a Python parser test alone is not a successful runtime result.

CI adds the proof-driver Python suite and one Linux proof step within the
existing workflow; it does not add a fifth job. Existing platform language and
JDT checks keep their established scope. The new popup still lacks positive
native/JDT and Windows typed-provider evidence.

## Documentation checks

The existing current-guide closure check passed for each of the Windows,
Linux agent and Linux desktop bundles using Python’s `-B` option: **three tests,
zero failures**. Each follows the packaged guide’s local links through the new
File Structure guide. The edited Markdown’s local targets resolve, and the
documentation diff passes `git diff --check`. These checks validate documentation
and package inventory closure; they do not establish app behavior, a built
release package or native acceptance.

## Accepted baseline and preserved limits

The accepted 0.66.2 public commit
[`a0ecbdbe27577ef83a60eee1b8af88edb0c68f35`](https://github.com/LLLLimbo/cedar-ide/commit/a0ecbdbe27577ef83a60eee1b8af88edb0c68f35)
passed all four jobs in
[CI 38392603729](https://github.com/LLLLimbo/cedar-ide/actions/runs/38392603729)
and three package audits. Linux recorded **2,128/0/70**, Windows **2,066/0/114**,
with 50 suites and 17 preview tests per platform. Browse **50 fields / 14
operations**, Scoped Search **ten operations**, Search **twelve operations** and
both JDT proofs were verified. Its bounded Linux native journey passed both
Text follow flows, bottom B160, Open/Back, dirty selection/Undo and
Escape/hide/reopen. Those results validate the existing baseline, not File
Structure.

The 0.66.0 bottom preview stayed pending for 37.223 seconds in its failed native
journey. The 0.66.1 Windows caller-count tests failed and its native run was never
launched. The 0.65.0 glyph stop and 0.64.0 late-quit timing failure also remain
preserved in the [verification report](TEST_REPORT.md). Screenshots do not prove
150 ms timing, operation counts or raw selection affinity. Windows/macOS GUI,
real SSH, arbitrary viewports, broad CJK/IME, full semantic navigation and IDEA
parity remain outside the established scope.
