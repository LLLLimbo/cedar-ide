# 0.65 Select Current File in Project candidate

**Final local checks passed; candidate not accepted. Exact CI, package and native
checks have not run.** The latest accepted bounded functional
baseline is **0.64.0**. Its exact source, CI, package checks and preserved strict
native timing failure are recorded in the
[accepted baseline](TEST_REPORT.md#accepted-0640-bounded-functional-baseline).
They do not validate this candidate.

## Candidate contract

The explicit, visible **Select Current File in Project** button below the
Flat/Tree controls selects and focuses the active file's exact visible Project
row while preserving the selected Flat or Tree mode. It may use existing List
only for the known parent or ancestor directories needed to expose that path.
It must reuse Explorer's single List slot. Flat admits at most **one List of the
known parent**; Tree admits at most **32 ancestor Lists per attempt**. Both modes
enforce one **fixed 30-second total deadline**, without renewing it after
progress. A busy List slot rejects
the attempt with an explanation rather than queueing it.

Existing directory-response and cache bounds remain in force. A result is not
success until the exact captured target has passed the final current-view row
identity and visibility check. Cancelled or stale intent cannot continue loading
ancestors or focus a row after a late reply. Cancellation does not promise to
interrupt an already-blocked filesystem call.

**Cancel selecting current file** remains visible and is disabled while idle,
so cancellation does not move the controls. The intent is conservatively retired
by meaningful new input, window blur, editor document/path/edit-version changes,
navigation, mode/connection changes and manual Project commands. Pointer
movement and the initiating key's matching release are inert. For an intent
already admitted from clean input, IME Enabled/Disabled lifecycle metadata alone
is also inert. Existing keyboard-entry classification is unchanged, and active
composition still blocks admission. Preedit/Commit events, including empty
payloads, and mixed meaningful input cancel pending intent. This is a preventive
alignment derived from source inspection, not a claim to have reproduced or
explained a native failure.

The existing
30-second per-List timeout remains unchanged: the independent fixed intent
deadline includes final rendered-row selection. An admitted ordinary List can
settle through its existing guards after cancellation or expiry, but cannot
continue or focus the retired intent. In Flat mode that List may still replace
the displayed parent directory. No rollback of admitted directory work is
promised.

Only exact known parent or ancestor directories are listed; this performs no
recursive or project-wide scan. No content Read, discovery or search operation,
automatic follow-active-file behavior, protocol change or new backend operation
is part of this slice. Alt+1 retains its existing zero-request Project-focus contract.
The editor's dirty text, selection, Undo and location history, together with Find
state, must survive the action. See the
[packaged usage guide](FIND_AND_RETURN.md#select-current-file-in-project-065-candidate).

## Local validation checkpoint

The final 0.65.0 Linux workspace/all-target/all-feature/locked aggregate passed
**2,109 tests / zero failures / 70 ignored across 50 suites**, including **27 new
Project selection tests**. The Python aggregate recorded **792 total tests /
zero failures / 10 skipped across 28 suites**. Strict Clippy with all targets
and features, including `-D warnings`, and the final formatting check passed.

The same-source aggregate, strict Clippy and formatting reruns all passed after
the test-only normal-agent proof correction described below and the preventive
IME lifecycle refinement.
Exact-commit CI, all three exact-source package audits and a fresh
native journey **have not run**. Earlier 0.64 evidence cannot substitute for
these candidate checks or acceptance.

## Normal-agent process proof and preserved failure

The first local normal-agent attempt failed while probing whether Cancel was
enabled immediately after starting selection. Inspection established that the
pinned egui response reflected its previous pass. The proof now permits exactly
**one idle production frame** before probing the enabled Cancel control. This
changed no production behavior and did not weaken the assertion. The original
failure remains part of the record.

A fresh current app binary selected from Cargo JSON and the matching normal
agent passed the strict Browse validator after that correction; the test
reported **0.41 seconds**. This is a runtime test result, not a comparative
performance claim. Its receipt is **version 3, 12 operations and 38 fields**,
with **zero additional Reads** and **one agent reaped**.

The proof retains the existing 29 field names and assertions, with the updated
receipt version, and the original eight-operation ledger prefix. Three exact
ancestor Lists and one cancelled root List add four operations; nine new fields
record the new contract. Existing driver and runtime bounds are unchanged.
The retained scoped-search proof also passed its strict validator with the
current artifact, retaining **ten operations**; its test reported **0.45 seconds**.

These are direct actual-runtime proofs with strict receipt validation. The
canonical compile/release driver was **not run locally**. They do not establish
exact-source package, CI or native GUI acceptance.

Windows/macOS GUI, real SSH, arbitrary viewport behavior, IDEA parity and
comparative performance remain unestablished by local candidate checks.
