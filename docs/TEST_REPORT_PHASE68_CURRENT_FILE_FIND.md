# 0.68 current-file Find — candidate verification

## Status and accepted baseline

**0.68.0 is a candidate and is not yet accepted.** Versioned local
aggregate/strict checks, independent input review and four current-artifact
process proofs passed. Final candidate sealing, exact dual-platform CI, three
package audits and any bounded native validation remain pending separate gates.
Local verification cannot stand in for those remaining gates.

The latest accepted baseline is **0.67.1** from
[CI 38431261412](https://github.com/LLLLimbo/cedar-ide/actions/runs/38431261412).
All four jobs and three package audits passed. File Structure positive behavior
is covered by headless frame checks and the Linux normal-agent/mock proof;
shipping native evidence covers only Trust-off visible menu entry, the
unavailable-provider explanation, filter, Escape/reopen and dirty-selection
Undo/Redo. Positive native symbols, real-JDT File Structure GUI and native
Ctrl+F12 remain unmeasured. The
[0.67 report](TEST_REPORT_PHASE67_FILE_STRUCTURE.md) preserves the native entry
failure and independent raw-hook observation without assigning an upstream cause.

## Bounded candidate contract

The [offline Find guide](FIND_AND_RETURN.md#current-file-find-loop-068-candidate)
contains complete usage and examples. This slice searches the active draft,
including unsaved bytes. It adds no filesystem or backend operation, language
provider, protocol change, automatic Trust, Save or global highlight.

- Ctrl/Cmd+F from the editor copies the whole valid nonempty selection, limited
  to 4,096 UTF-8 bytes without CR, LF or NUL. An invalid seed preserves the old
  query and gives a reason; no selection retains it. Manual query rules remain
  unchanged. Opening Find or typing its query does not move the editor caret.
- F3, Shift+F3, Next/Previous and query-field Enter share literal,
  case-sensitive, non-overlapping occurrence navigation. An exact selected hit
  is skipped in the chosen direction; otherwise Next uses starts at or beyond
  the higher selection endpoint, and Previous uses ends at or before the lower
  endpoint. A collapsed caret on a start/end boundary includes the match in that
  direction; an interior caret skips it before wrap.
- Cache identity includes exact document, edit version, query and connection.
  At most the first 10,000 hits are retained. A full cache is labelled as a
  retained range rather than a complete total, and wrap remains within it.
- The first successful move adds one Back departure for a contiguous loop.
  Edits, tab/query/connection changes, external navigation, manual editor
  selection changes and Escape end it. No-hit and an already-selected sole-hit
  wrap add no history. Ordinary Back validity checks still apply.
- Dirty text, Undo/Redo and explicit Replace preview/Apply keep their existing
  boundaries. Current-file Find leaves the retained Project Text snapshot
  unchanged. Original-event ownership, held keys, IME, stale pointer context and
  foreign focus must not retarget a command.

## Evidence recorded so far

| Gate | Result and limit |
|---|---|
| Early focused checks | **110/110 passed.** This is the early development result, not the final versioned workspace aggregate. |
| Browse proof parser checks | **15/15 passed.** Strict version-5 schema retains the original 50 fields and adds seven typed fields, rejecting historical or malformed receipts and extra operations. Parser tests alone are not process evidence. |
| Early current-artifact normal-agent Browse execution | Passed on the first execution with **57 strict fields / 14 operations / zero added current-file operations**, one agent reaped, Trust off and unchanged fixture bytes. The selected test passed **1/0/0 in 0.72 s**. These default-feature app/agent artifacts still reported development version **0.67.1**; this is not a final 0.68 runtime result. |
| Documentation and offline guide closure | The three existing current-guide closure tests passed for Windows, Linux agent and Linux desktop bundles. Relative Markdown targets in the edited guides/reports resolve; no package inventory expansion was needed. These are documentation checks only. |
| Versioned local aggregate and strict checks | **0.68.0: 2,176 passed / zero failed / 71 ignored across 50 Rust suites**, using workspace/all-target/all-feature tests. Python: **804 total / zero failures / ten skipped across 29 suites**. Strict workspace/all-target/all-feature Clippy with warnings denied and formatting passed. |
| Current default-artifact process proofs | Structure **45 fields / 11 operations**, Browse v5 **57 / 14 with zero added Find operations**, Scoped Search **57 / ten**, and Search **57 / twelve** passed. The selected current app, matching normal agent and controlled Structure mock were verified; production/test inputs remained unchanged through these executions. The canonical compile drivers were not run locally. |
| Independent source/input and proof review | Passed with no blocking findings, including the strict original-batch contract update and version-only addendum. The proof contract's independent static review also passed. These reviews do not replace exact CI, package or native gates. |
| Exact dual-platform CI and three package audits | Pending. |
| Native current-file Find journey | Not measured. No native shortcut, pixel, timing or platform acceptance follows from headless checks. |

The [Browse proof contract](FIND_AND_RETURN.md#current-file-occurrence-process-contract-068-candidate)
keeps the original fourteen-operation ledger and strict existing bounds. It
reuses the ordinarily opened dirty draft and existing Undo history, then checks
selection seeding, exact Unicode/CRLF occurrence ranges, next/previous wrap,
literal and case-sensitive no-match queries, Escape/Back exact return, state
preservation and actual Undo/Redo. Every new current-file action must add zero
backend operations. Source and fixture integrity, agent cleanup and receipt
validation are required independently of process exit status.

## Preserved development failures and corrections

The first versioned full aggregate stopped on an old mixed Ctrl+J/Ctrl+F
expectation: its app test binary recorded **1,459 passed / one failed / 54
ignored**. The former positive expectation conflicted with the approved strict
original-input-batch Find contract. The revised test requires that the mixed
batch preserve the Find panel, query, editor focus, dirty draft and zero
operations, then verifies a following clean Ctrl+F focus transfer. This is an
intentional contract change, not merely a fixture correction. The failed run
remains failed; the replacement aggregate is reported separately above.

Earlier focused work exposed a real IME availability-notification defect: the
newly selected range collapsed during focus transfer. Production now discards
obsolete keyboard state only after a validated move, and the final focused
positive checks retain the exact range assertion. A separate Replace test
incorrectly appended `cat` to a query now automatically seeded as `cat`. That
fixture now selects all text in the actual Find field before entering the query;
the original Replace protection assertions remain.

The former blanket expectation that all Find actions were history-free also
changed deliberately: explicit occurrence movement now records one departure,
while query typing, ordinary caret movement and Undo do not add Find history.
A separate mixed Ctrl+F/text test now keeps text with its original editor owner
and checks clean opening before typing. These contract changes are distinct
from removing an obsolete fixture for the replaced internal match index.

Independent input review also required removal of accidental new document and
manual-query size caps, rejection of repeat-marked key-release edges, and
retirement of obsolete ready navigation before applying a new Find command.
It checked interrupted modal/focus ownership and the unchanged shared input
parser. These review corrections preserve the approved seed-only byte limit
and current-owner command boundaries; they do not establish native input delivery.

## Remaining scope limits

Candidate results must be attributed to the actual artifact and version tested.
No current-file native Windows/macOS journey, real SSH, full IME/CJK coverage,
arbitrary viewport coverage or performance improvement is established here.
Existing native File Structure negative-path acceptance does not validate this
Find change. Project Text remains a separate explicit disk search; no regex,
case/word option, multi-file replacement, language feature or persistent search
state is added by this candidate.

The preserved 0.67 native entry failure, 0.66 preview/test failures, 0.65 glyph
stop and 0.64 strict timing failure remain in the
[current report](TEST_REPORT.md); they are not reclassified by this work.
