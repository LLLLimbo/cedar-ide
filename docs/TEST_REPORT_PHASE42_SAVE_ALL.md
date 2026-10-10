# Verification report · Explicit Save All / 0.42.0

The latest fully accepted checkpoint is [0.41.2](TEST_REPORT_PHASE412_LINUX_DESKTOP.md). This increment adds an explicit sequential save of at most 32 dirty editor buffers, bounded to 1 MiB each and 32 MiB total, using the existing conditional Write and exact acknowledgement/recovery pipeline. Candidate metadata is retained; the cohort does not clone every draft. Fresh exact-source CI, all three packages and scoped native desktop acceptance remain pending.

## Local source verification

Formatting and strict host/MSVC Clippy passed. The full all-features Rust workspace run passed 1,457 tests with 55 existing or separately exercised opt-ins across 52 suites. All 38 new Save All unit and actual-frame tests passed, including full 32 MiB admission, late edits and real Undo, exact modifier filtering, the 780px Save menu, cancellation/acknowledgement/EOF ordering, profile serialization and lifecycle guards, and synthetic Maven POM acknowledgement hooks. Python ran 451 tests: 442 passed and nine existing platform/tool-dependent cases skipped. Both desktop package guide inventories and local link targets include SAVE_ALL.md.

A defensive internal-state test removes or replaces the pending Save job after submission, followed by either a reply or passive transport loss. The exact original batch owner then remains session-only unverifiable; no token is invented, no later Write is sent, and other paths/sessions are untouched. An actual-frame Undo-to-clean/Cancel test drops the app immediately and verifies the owned recovery record remains on disk without another frame. This is internal-fault robustness, not an observed normal-peer failure or a claim that uncertainty survives application restart as a serialized flag.

## Actual local process acceptance

Fresh default release frontend and agent binaries were built, followed by the separate nonshipping interrupted-save fixture. Their hashes remained unchanged around fixture compilation and all process checks. Four Save All tests actually executed, yielding five fixed receipts:

| Case | Requests | Conditional Write attempts | Result |
|---|---:|---|---|
| Normal agent, three files | 11 | 1 / 1 / 1 | Three submitted snapshots acknowledged; newer draft retained |
| Normal agent, second-file revision conflict | 8 | 1 / 1 / 0 | First saved, second failed, third unattempted |
| Controlled second wrong digest, explicit read-back check | 10 | 1 / 1 / 0 | Original second outcome unknown; no replay or resumption |
| Controlled second lost reply, reconnect and read-back check | 12 | 1 / 1 / 0 | Original second outcome unknown; no replay or resumption |
| Controlled fixture boundaries | 4 accepted protocol requests | Three rejected attempts, zero commits | Eight rejected handshakes plus fixed-path/conditional-write guards |

The four main cases verify actual source bytes, exact conditional revisions, full selections, Undo/Redo, owned recovery and cleanup. All successfully established Client owners were reaped. Reaping of rejected handshakes was not independently observed and is not claimed. Run and language operations were zero; real Maven or JDT execution is not part of this file-saving gate. The POM hook proof is separately identified synthetic frontend coverage.

The unchanged legacy interrupted-save reconciliation suite passed all nine actual process cases, and the prior acknowledgement-protection suite passed all five. CI requires the controlled cases on both OSes and the normal cases again against each freshly rebuilt default shipping agent, preserving binary hashes. All prior required Java/Maven, ownership and package gates remain required.

## Remaining acceptance and limits

The new Ubuntu-built desktop archive must be independently verified before the cloud Debian X11 trust-off GUI flow. That flow covers three generated files, Save All menu/shortcut accessibility and a newer draft with Undo, with exact source/payload hashes and owned window exit. Native in-flight typing or pending Cancel is not claimed unless actually observed; deterministic frame/process tests cover those races. No GUI execution-trust change, user-device operation, real SSH, new capability, dependency download, resource optimization or broad platform-parity claim is made.

Only captured submitted contents count as acknowledged. Later typing remains dirty. Cancellation stops unsent files and retains ownership of any in-flight Write. The operation is not multi-file atomic and never rolls back, retries, auto-runs, serializes a profile form, or resumes after reconnect. Identity-unavailable outcomes preserve the draft and block both Check and further saves; valid captured identities retain the existing explicit two-read reconciliation route.

## Final exact-source acceptance

Public source `16965ebc65123d6bc83f75dd5fc00dbb40ef1185` passed both OS jobs in [CI 38066217638](https://github.com/LLLLimbo/cedar-ide/actions/runs/38066217638). Both OSes executed all 38 Save All unit/frame tests and the five process receipts, including the fresh default shipping-agent cases. All three package archives and their payload/source/provenance checks passed.

The exact Ubuntu-built desktop passed the scoped cloud Debian 13.6 X11 trust-off native flow: three generated files matched expected saved bytes; a newer draft and selection survived tab switching; one Undo returned to the saved state; the window closed normally with frontend exit 0. All 527 payloads plus the manifest remained unchanged. Native pending Cancel, in-flight typing, Save All shortcut, Java/Maven execution and independent agent reaping were not exercised. The dropdown glyph appeared as a hollow square, though its menu was readable and clickable; semantic accessibility exposed only a generic X11 window.

Linux Java preserved a failed spontaneous diagnostic wait, then matched after exactly one permitted explicit refresh (workflow success, 84.444 seconds). Initial Stop was graceful exit 0; restart Stop was forced SIGKILL 9 with joined cleanup. Windows Java corrected spontaneously. Linux and Windows Maven pairs passed in 24.971 and 34.999 seconds. These observations do not establish an upstream diagnostic fix.
