# Case-aware Project Text: Stage 3 R2

This is a new implementation from public Stage 2 commit
`52b41427d119355b6de2c3e7f5166aad29ef5d6f`, tree
`c6bbc7315d12e25d5e76f39edfba72d9f7efd47c`. The prior final local Stage 3
source and evidence were lost when its cloud environment reset. Six genuine
early, unvalidated source blobs were recovered and reviewed as partial
references. They do not recover the prior final tree or transfer its test results.
Every R2 validation result must be tied to the actual new source bytes.

## Shared draft and routing

Find in Files and the bottom Find Results tool share a Match case checkbox.
It starts checked and remains an in-memory draft through provider switches,
hide/reopen, reconnect and workspace changes. It is not persisted across restarts.

Checked root/no-mask requests keep the existing Search route. Checked filtered
requests keep SearchIn v1. Unchecked requests require the independent exact
case_search v1 descriptor and always send SearchCase with case_sensitive:false,
with or without directory/masks. This requires no execution trust. Unsupported
modes do not change the checkbox, erase filters, broaden the scope, retry through
another operation or automatically submit. The unavailable message is:

“Case-insensitive search is not available from this agent. Turn on Match case to
use its existing search, or upgrade the agent.”

Matching remains the Stage 2 literal Unicode simple-fold contract. There is no
whole-word or regex UI. Exact directory paths and basename masks remain
case-sensitive. Editing or toggling never sends or queues a search.

## Captured authority

A specification captures the query, scope, ordered parsed masks, requested limit,
case flag and explicit Legacy/ScopedV1/CaseV1 route. Case responses validate the
complete echoed request before any row or snippet is accepted. Historical rows,
counts and summaries retain the accepted mode rather than the current draft.

A genuine edit/toggle retires results, pending reply authority, preview/review and
search-origin Open authority. Toggle-back does not revive it. Physical jobs stay
occupied until they drain; there is no backend cancellation. SearchOpen retains
its exact source, row and selected index through Read. Retirement leaves generic
Open, same-path physical reuse, dirty buffers, navigation history and safe inactive
caching intact. A matching retired Read remains POM disk evidence even if an
existing draft or cache bound prevents insertion.

## Native control and geometry

Both controls use native egui focusable checkbox semantics and standard painting,
with mutation only after original-input admission. Their measured rectangle fits
in the existing query/submit band, preserving prior result and preview heights.
Native registration order is query, directory, masks, Match case, submit.
The checkbox-local tooltip uses pointer placement to avoid covering its own target.

Space/Enter activates only the checkbox. A checkbox-origin gesture, including a
rejected mixed batch, cannot also submit or activate a result. Held key ownership
survives focus/view changes until physical release, including its bounded exact
Space text companion. Fresh editor input after release remains ordinary input.
IME content and unrelated text are not consumed as checkbox companions. Ordinary
query typing plus Enter retains each surface's established behavior.

## Verification boundaries

Deterministic raw-input/update tests use recording workers and actual production
egui controls. They do not establish native OS GUI, real SSH, performance or a
backend application journey. Normal-agent client tests are separate process and
protocol evidence. Stage 4 application ledgers and package/native journeys remain
separate work. Source-only UNVALIDATED checkpoints are backup milestones, not test
passes or release claims.

## Explicit test-only boundary-contract exception

Two test files outside the app are deliberately changed, with no producer or
resource-limit changes. The maximum escaped 4,096-byte literal now permits only
a validated TimeLimit response containing either no rows or the exact expected
row, at most two examined entries and 4,096 read bytes. Complete results must be
exact; all errors and other incomplete reasons still fail. Full request echo
validation remains mandatory. A short literal must complete immediately afterward
on the same connection in each case mode. The direct matcher test retains full
matches under production component caps and additionally checks original UTF-8
byte offsets after a six-byte multibyte prefix.

The rationale is the documented cooperative budget, not a performance fix. The
previous lost run was reported to fail a strict maximum-escaped follow-on; its
raw log is unavailable, so R2 does not claim to recover it or identify its case
mode, counters or cause. R2's fresh external diagnostic recorded all three maximum
requests completing, including each escaped mode, with exact echoes. That new
observation also establishes no unconditional latency promise. New full tests and
raw evidence remain required for this revision.
