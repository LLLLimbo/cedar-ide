# 0.65 Project selection and accepted 0.65.1 search CJK correction

## Accepted 0.65.1 bounded baseline

**0.65.1 passed its bounded functional and timing acceptance.** Public commit
[`bc65427bb1fc2c0da3b7391c65a8340ed0619db4`](https://github.com/LLLLimbo/cedar-ide/commit/bc65427bb1fc2c0da3b7391c65a8340ed0619db4)
passed all **four jobs** in
[CI 38371007337](https://github.com/LLLLimbo/cedar-ide/actions/runs/38371007337)
and all **three exact-source package audits**. Each platform ran **50 suites**:
Linux **2,111 passed / zero failed / 70 ignored** and Windows **2,049 passed /
zero failed / 114 ignored**. Both new CJK tests and all **27 Project selection
tests** passed. The strict Browse proof retained **version 3, 38 fields / 12
operations**; scoped search retained **ten operations**, Search retained
**twelve operations**, and the required JDT checks passed.

The fresh bounded Linux native journey displayed `src/层/Target.rs` readably
and passed Tree/Flat current-file selection, Down/Up, dirty-selection and
Undo/Redo preservation, and retained Find checks. Startup was **30.994 seconds**
against the 90-second bound. Quit began **303.840 seconds after visible
Connected**, before the 550-second wind-down and 600-second case limits.
Exit code **0** arrived **12.579 seconds** after quit initiation and the window
was absent at **12.703 seconds**, both within the 120-second shutdown bound.
These are observations of that bounded run, not general performance guarantees.

Native cancellation, Windows/macOS GUI, real SSH, arbitrary viewports and
broader CJK/IME coverage remain unverified. No font download or installation was
added. The stopped **0.65.0** glyph journey below remains a failed earlier run.
The **0.64.0** quit-initiation delay of **20.436 seconds**, and its strict timing
failure, remain preserved in the
[prior baseline](TEST_REPORT.md#accepted-0640-bounded-functional-baseline).
The following local preparation evidence predates this final checkpoint and
does not validate the separate 0.66 candidate.

## Preserved 0.65.0 CI, packages and stopped native journey

Public commit
[`ec8428a020acc58e47eb5c722de331fcb2b55a03`](https://github.com/LLLLimbo/cedar-ide/commit/ec8428a020acc58e47eb5c722de331fcb2b55a03)
passed all **four jobs** in
[CI 38365574615](https://github.com/LLLLimbo/cedar-ide/actions/runs/38365574615)
and all **three exact-source package audits**. Linux recorded **2,109 passed /
zero failed / 70 ignored** and Windows **2,047 passed / zero failed / 114
ignored**, each across **50 suites**, including **27 new Project selection
tests**. The strict Browse proof retained version 3, **38 fields / 12
operations**, all pre-existing field assertions and the original eight-operation
ledger prefix. The retained scoped-search and Search proofs kept **ten** and
**twelve operations**, respectively.

The Linux Java restart proof reported forced cleanup after `grace_expired`,
with **SIGKILL (signal 9)**. Its result satisfied the existing cleanup predicate;
this is not evidence that both Java stops were graceful.

The fresh Linux native journey stopped early when Project Text displayed a box
in the result path `src/层/Target.rs`. Fixture UTF-8 bytes remained intact; this
display observation does not establish wrong-path selection. A
**19,484,784-byte** Noto CJK collection was already installed at a supported
fixed location; a missing download was not established as the cause. The
journey did **not** exercise the core current-file selection, dirty-selection
preservation, Down/Up or Undo checks. The application exited safely with code
**0**. Neither the stopped run nor the green CI/packages establishes native
acceptance of 0.65.

## Accepted 0.65.1 font-trigger contract

Source inspection found that Project Text result acceptance did not mark CJK
text as seen, unlike the existing filename-discovery and Explorer acceptance
paths. The correction marks CJK found in accepted result **paths or snippets**
only after the existing current-request and response validation gates pass.
Both ordinary and scoped Project Text results use the retained result snapshot.
Rejected, stale or invalid responses must not acquire this trigger.

The change reuses the existing background font loader: known local font
locations, one validated regular file of at most **32 MiB**, and the existing
fallback families. It adds no font download, installation, dependency, protocol,
backend operation or broader filesystem scan. Search bounds, result identity
and navigation behavior remain unchanged. Finding a font does not guarantee
that it contains every CJK glyph. The bounded native rendering observation above
confirms the tested path, not every CJK string or font environment.

## Preserved 0.65.1 local validation checkpoint

The final Linux Rust aggregate passed **2,111 tests / zero failures / 70 ignored
across 50 suites**. The Python aggregate recorded **792 total tests / zero
failures / 10 skipped across 28 suites**. The affected focused set passed
**117 tests / one ignored**. Two new tests cover a **12-case positive/control
matrix** and **14 rejection cases**. Independent source review passed.

The first focused fixture failed because it captured the epoch baseline before
legitimately opening Browse. The fixture now captures that baseline after
opening, immediately before the response frame. This correction changed no
production behavior; the original failure remains part of the record.

Strict Clippy and the final formatting check passed. The final 0.65.1 app
artifact selected from Cargo JSON and the matching normal agent passed the
unchanged strict Browse validator: **version 3, 38 fields / 12 operations**,
reported runtime **0.43 seconds**. The retained scoped-search proof passed its
unchanged strict **ten-operation** validator, reported runtime **0.48 seconds**.
App and agent identity were verified. These direct current-artifact runtime
proofs are not comparative performance measurements; the canonical
compile/release acceptance drivers were **not run locally**.

At this preparation checkpoint, exact CI, three package audits and fresh native
validation were still pending. Their later accepted results are recorded above;
local evidence alone did not establish those gates or corrected rendering.

## Retained Project selection contract

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

## Preserved 0.65.0 local validation checkpoint

The final 0.65.0 Linux workspace/all-target/all-feature/locked aggregate passed
**2,109 tests / zero failures / 70 ignored across 50 suites**, including **27 new
Project selection tests**. The Python aggregate recorded **792 total tests /
zero failures / 10 skipped across 28 suites**. Strict Clippy with all targets
and features, including `-D warnings`, and the final formatting check passed.

The same-source aggregate, strict Clippy and formatting reruns all passed after
the test-only normal-agent proof correction described below and the preventive
IME lifecycle refinement.
The later exact-commit CI/package results and stopped native journey are
recorded above. These 0.65.0 local results do not validate the 0.65.1 font-trigger
change; its own local and final checkpoints above record that evidence separately.

## Preserved 0.65.0 normal-agent process proof and failure

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
native GUI acceptance or validation of 0.65.1. The later 0.65.0 CI and package
results are separate evidence recorded above.

Windows/macOS GUI, real SSH, arbitrary viewport behavior, IDEA parity and
comparative performance remain unestablished by these checks.
