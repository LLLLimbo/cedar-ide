# Verification report · Capability inventory correction / 0.29.1

This is a test-inventory correction to the dependency-insight checkpoint. No Rust
product behavior, capability advertisements, trust policy, model schema, native
acceptance condition or timeout is changed.

## Preserved failure

[0.29.0](TEST_REPORT_PHASE29_DEPENDENCIES.md) passed Ubuntu but failed Windows
aggregate verification: its exact allowed capability set already matched all 31
actual names, while a duplicate cfg-only numeric assertion still expected 30.
Native dependency acceptance and the Windows package did not execute. That run
remains failed; it is not evidence of a Maven runtime failure or success.

## Correction and regression

The actual Hello test continues to require exact allowed-set equality and the
32-capability wire ceiling. Its exact count follows from that equality instead
of a second numeric literal. A shared independent test inventory derives from
the explicit operation list and platform policy. A host-runnable regression
checks Linux, macOS, Windows and other targets under both backend modes, including
unique names, forbidden generic Windows routes, optional Maven availability,
remaining wire capacity and one-over-limit rejection. It does not simulate
actual Windows process execution.

Read-only audit of the normal isolated agent, nonshipping fixtures, protocol,
client, Python checks and extracted-bundle inventory found no other stale count.
Current Windows normal inventory has 31 entries, leaving one slot under the
unchanged limit. Future growth must explicitly account for that finite capacity.

## Current verification

Independent inventory/patch review has no remaining findings. All four focused
capability tests pass on the host, including the all-platform matrix. Strict
host and Windows MSVC-target Clippy pass. The all-target host aggregate completed
41 suites: 1,135 passed, zero failed and 29 explicit opt-ins ignored. All-feature
and default shipping release builds pass; the normal Linux agent capability/trust
smoke passes with 24 capabilities and no tool startup. Exact-source dual CI,
actual Maven present/missing dependency receipts and every package payload hash
remain required. [0.28.1](TEST_REPORT_PHASE281_BUNDLE_GUIDE.md) remains the latest
fully verified distribution. Its original spontaneous Java timeout and successful
one-refresh recovery remain separately recorded; no upstream fix is claimed.
Windows native GUI and authenticated SSH remain unverified.
