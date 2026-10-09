# Verification report · explicit conservative draft merge / 0.26.0

This checkpoint extends Compare with disk with an explicit preview and draft-only
merge for strictly separated textual change regions. It adopts the reviewed disk
baseline without marking the merged draft saved. See [scope](DRAFT_MERGE.md).

The previous 0.25 public source `217572610a4f7122eeb7c47772ec8ff533c4b899`
passed [exact Ubuntu/Windows CI 37903752630](https://github.com/LLLLimbo/cedar-ide/actions/runs/37903752630).
Both platforms executed the normal-agent idle acceptance with all ten witnesses and
all six worker process cases. Its verified Windows package contained 532 payloads
and a manifest. The historical report remains [here](TEST_REPORT_PHASE25.md).

## Finite acceptance

- Linear bounded exact-line merge; ambiguous, touching and overlapping regions
  are refused. Unicode, repeated lines, newline variants and zero-width edits
  have adversarial coverage. No general or semantic merge claim.
- Preview is inert; Apply uses one fresh Read and a final-frame identity/selection
  barrier. Missing, changed, malformed or stale responses do not mutate drafts.
- One Undo restores the original draft and full selection. The newly observed disk
  text/revision remains the baseline through Undo/Redo; merge itself emits no Write.
- Recovery ownership and unowned older drafts remain intact. Task-profile drafts
  are retained while queued mutations and outdated profile sources are invalidated.
- Trust-off normal-agent acceptance runs on both OS: draft merge without Write,
  later explicit Save with the new revision, conflict after another external edit,
  and refusal when disk changes between preview and verification.

## Current verification status

The host aggregate passed 1,022 Rust tests across 40 suites, with 29 opt-in tests
ignored. New coverage includes 12 pure-engine tests and 13 transaction tests; the
independent minimum-envelope oracle checked 29,791 Unicode/newline triples.
Actual egui pointer Preview/Cancel/Apply and full-selection Undo/Redo passed, as did
same-frame refusal, profile queued-action invalidation and recovery-store ownership.

Strict host and MSVC all-target/all-feature Clippy passed. Cross-compilation is not
Windows execution. Independent algorithm and transaction/recovery reviews found no
blocking issue; exact line-cap parity and selection-affinity assertions were refined.
Python checks passed: export 2, capabilities 5, Git fixture 6, Maven cache 11, resource
observer 79, GC collector 36; collector 77/81 (four platform/tool skips), bundle 28/29
(one skip), Maven predicates 5/7 (two skips).

The all-feature release built successfully. Actual Linux normal-agent acceptance
passed three generated trust-off cases: draft-only merge and later explicit Save,
an intervening external edit rejected by the later Save revision check, and changed
disk bytes refused during Apply verification. All 13 fixed witnesses passed, including
no merge-generated Write, separate disk baseline, full-selection Undo/Redo, exact
owned-agent reaping and expected final disk contents. The default-feature release
frontend and agent also built successfully; the same three-case process acceptance
passed against that shipping agent. All 35 disk-review tests passed on the final
source after the last help-text correction.

Exact native CI and regenerated package hashes remain required before acceptance.
This is not filesystem identity, atomic compare-and-swap, native GUI or authenticated
SSH validation. Existing diagnostic and remote-transport limitations remain.

## Exact native result

Public `723bd3a348a66515ebc650aafa4f0afc1bd0517c` ran [CI 37907826294](https://github.com/LLLLimbo/cedar-ide/actions/runs/37907826294). Ubuntu passed. Both platforms executed the three-case merge acceptance with all 13 witnesses true. Windows failed the historical second long Java resource trial: correction was acknowledged but no accepted correction diagnostic witness arrived; total trial time was 99,801 ms. Earlier semantics and editor assertions passed, source remained unchanged, and retained-root natural exit 0, joined cleanup and client reaping passed. This receipt does not establish zero events or the cause of the missing witness. The run remains failed; no Windows development ZIP was produced.
