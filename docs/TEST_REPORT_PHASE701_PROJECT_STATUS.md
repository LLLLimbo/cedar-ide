# 0.70.1 Project status wording — candidate verification

## Scope and status

This is a wording-only correction on the 0.70.0 source snapshot
[36a63c71e10d0a9fa3be9a3e482bb55ff2a065d6](https://github.com/LLLLimbo/cedar-ide/commit/36a63c71e10d0a9fa3be9a3e482bb55ff2a065d6),
tree `8f271f4c1f5349f220d50b9c467684eae83cb958`.
The 0.70.1 candidate is not accepted until its exact-source CI, three package
audits and bounded Trust-off native wording regression are complete.
Historical reports retain their original checkpoint and failure classifications.

The successfully completed Select Current File in Project action now says
`Selection completed: /path`. Its target comes from the validated reveal intent,
not a later active document. Only this generated success status uses the existing
single-row Label truncation; pinned egui retains the complete UTF-8 string for
its automatic hover text. Pending, cancellation, failure and other statuses
keep their existing wrapping and replacement behavior.

No file selection, scope, cache, request, input, focus, cancellation, deadline,
navigation lifecycle or history rule changes. The production diff is confined
to the success string and its narrow display bound. There is no new feature,
backend operation, provider, protocol, dependency, package, job or Trust behavior.
Thirteen existing version-coupled files move from 0.70.0 to 0.70.1; external
locked packages are unchanged.

## Old-production negative control

The actual raw-input-hook/update frame journey started with a dirty origin
buffer, a nonempty selection and both populated native Undo/Redo branches.
The Chinese Flat directory was already loaded with Long and Needle entries.
A real Select button selected Long, then Recent reactivated the existing origin.
The active origin, unchanged scope/list, retained Long selection, full dirty
witness and zero additional worker requests all passed before the old success
message failed the exact historical-target assertion.

The old text was `Current file selected in Project`; the initial accurate
expectation was `Selected in Project: /中文/Long.rs`. The final wording was made
still more explicit as `Selection completed: /中文/Long.rs`. The old-production
negative control covers the Recent route; the loop stops at that intentional
failure, so it does not establish old-production tab/Back results. Both later
routes are covered on the corrected source.

An earlier setup failure baselined origin before Recent could observe its first
activation. That fixture failure and source/log remain preserved separately;
it is not evidence of a product defect. The corrected fixture registers origin
through the normal observer before the measured journey. No test globally clears
the status to obtain a pass.

## Final source-bound local checks

The final 0.70.1 code passed Rust formatting, strict workspace/all-target/
all-feature Clippy and the locked/offline workspace/all-target/all-feature test
aggregate: **2,206 passed / zero failed / 71 ignored across 50 suites**.
Python passed **806 total / ten skipped / zero failed suites across 29 suites**.

Three added production-frame tests cover:

- Real Select Long followed by Recent, tab or Back to dirty origin, with exact
  retained scope/list/target, zero new operations and preserved text, revision,
  version, selection affinities and both native Undo/Redo branches.
- A long CJK target at a 780×540 viewport with 13- and 18-point text, exactly one
  elided status row, visible clipped geometry and a complete retained path.
- A later selection cancellation, failed directory request or mode change
  replacing/clearing the previous historical success through existing behavior.

The original cached-success/missing test now asserts both exact outcomes and
selection state. The discarded-pass and unrendered/zero-viewport tests assert
exact pending text and a live pending intent, rather than merely rejecting the
retired success literal. The earlier focused run passed **30/0/1** while the
success phrase was still `Selected in Project`; the final aggregate above uses
`Selection completed` and covers those same tests.

## Process proofs and external gates

The retained Browse v7, File Structure, Scoped Search and Search process proofs
all passed using artifacts selected from current default-feature Cargo JSON, bound
to the complete final non-document source. Their unchanged contracts are Browse
**74 fields / 17 operations**, Structure **45/11**, Scoped **57/10** and Search
**57/12**. Structure uses the explicit mock. Local canonical validators and exact
ignored proof tests run; the canonical compile drivers are not run locally.

Exact-source CI, Linux/Windows/macOS package audits and the requested bounded
native wording regression remain pending at this source checkpoint. Supplied
frame events are not native user-input or pixel evidence. Nothing here broadens
Windows/macOS GUI, real SSH, broad IME/CJK, positive Structure/JDT GUI or other
platform guarantees. All previous evidence and failures remain retained.
