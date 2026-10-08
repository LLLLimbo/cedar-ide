# Verification report · interrupted-save reconciliation / 0.10.0 · 2026-10-08

This checkpoint adds an explicit, read-only way to check a save whose reply was
lost. Exact native CI for the new checkpoint is pending.

## Verified public baseline

Exact public 0.9.0 commit
[`270119b45eea1d37581a497e1bbe9a2d4ba3764a`](https://github.com/LLLLimbo/cedar-ide/commit/270119b45eea1d37581a497e1bbe9a2d4ba3764a)
[passed Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37727806345).
Normal shipping-agent/Client Java acceptance passed capabilities, trust rejection,
source semantics, actual editor transactions and native process identity checks.
Production Stop was accurately reported as forced, with grace_expired and root
exit1067; both protocol witnesses, joined cleanup, verified client reap, unchanged
source and fixture removal passed. That is not a natural production-exit claim.
The prior strict direct/fixture three-session tests, real task independence and
forced-agent ownership cases also passed.

## Interrupted-save behavior

A save can commit remotely before its Written reply is lost. Previously the
frontend retained the old revision but discarded the submitted snapshot; a later
Save conflicted, while dirty-file comparison could not adopt the current base.
The new session-local token records a bounded submitted-content fingerprint and
original workspace/path/tab/request/baseline lineage. It never asserts that an
accepted but unanswered request was transmitted or committed.

A successful Check interrupted save requires two agreeing Read responses; an
earlier failure can stop the check after one. Each
returned revision must equal SHA-256 of its actual text. A match with submitted
contents can update only the saved baseline/revision; newer typing, native history,
cursor and edit version remain intact. Original-baseline matches merely resolve
current uncertainty. Original new-file absence requires two not-found reads and
still needs a later explicit Save to create anything. Existing-file absence,
divergence and malformed responses cannot silently adopt a new base or write.

Generation, navigation, current-content, edit-version, profile and close-action
guards reject stale results, including same-frame input. Duplicate checks and
save/check overlap remain bounded. Recovery updates use existing ownership and
storage-acknowledgement guards; profile edits and reconnect review remain intact.

## Deterministic acceptance

A separate feature-gated fixture requires an exactly marked generated root,
rejects execution trust and accepts only Hello, root List and draft.txt Read/Write.
It uses the real Workspace write implementation, then exits after the first
successful commit without sending Written. A later process reads the same bytes.
Its bounded operation ledger verifies that reconnect and checking send no Write.
The fixture is not an ordinary agent flag or a published product binary.

Nine explicit stdio acceptance cases cover committed lost replies, unchanged and
newer drafts, native Undo/Redo, diverged/missing/recreated files, stale checks,
malformed path/revision/content, subsequent explicit Save and recovery ownership.
Focused tests additionally cover original absence, profile forms and native-frame
input ordering. The CI workflow runs the process cases on both operating systems;
leaving ignored tests unexecuted is not a pass.

## Evidence boundary

The result means current disk contents match a known snapshot. It does not prove
historical commit provenance or physical-file identity; identical-byte recreation
is indistinguishable under the unchanged protocol. Transaction tokens are not
persisted across application crashes. Normal Written revision handling is not
redefined by this slice, and revision-based writes remain non-atomic compare-and-swap.

Authenticated SSH and native GUI Trust remain separately unverified. This change
adds no network service, credentials, automatic write/replay or execution trust.
See [user-facing behavior and limits](INTERRUPTED_SAVES.md). No private diagnostic
findings are included in this report.
