# Verification report · required idle Java workflow / 0.26.1

This checkpoint keeps idle Java correction behavior in required native acceptance
while separating the historical resource experiment into an explicit opt-in.
Production Java deadlines, heap, protocol and draft-merge implementation are unchanged.

The preceding [0.26 report](TEST_REPORT_PHASE26.md) records the successful new
three-case draft-merge acceptance on both platforms and the failed Windows long
resource trial in [CI 37907826294](https://github.com/LLLLimbo/cedar-ide/actions/runs/37907826294).
That failure remains a failure. An acknowledged correction without an accepted
push diagnostic is consistent with the documented limitation; its specific cause
has not been established, and that receipt does not prove zero events.

## Required idle workflow

The new nonshipping test uses the normal release agent and capability-enforcing
Client, an owned generated project and the existing semantic/editor workload.
It retains the initial 30-second quiet period and rapid Apply/Undo/Redo changes.
The original spontaneous diagnostic result remains separate from workflow success.
Only its timeout may trigger one user-equivalent typed diagnostic refresh.
A matching version-5 or unversioned synthetic warning is required; absent version
is disclosed and does not prove freshness or refresh causality.

The fixed envelope is 480 seconds: a 360-second primary cutoff and 120 seconds
reserved for cleanup. Primary Client calls require conservative 75-second admission
and a post-return deadline check. The original 60-second diagnostic dispatch window
may include a final 75-second in-flight call, so admission reserves 135 seconds.
Recovery requires 240 seconds before the primary cutoff: the existing 165-second
refresh/witness envelope plus 75 seconds for Close. There is no second measurement
idle in this behavioral test. Cleanup budgets total 111 seconds (Stop 75, retained
root checks 3 + 3, client reap 30), leaving nine seconds for bookkeeping.

A normal pass requires the original match and no recovery attempt. A recovered
workflow retains the original timeout/false result and requires exactly one
acknowledged refresh and the accepted synthetic witness. Insufficient budget,
late calls, malformed or closed event streams, request errors, failed recovery,
changed source or incomplete cleanup remain failures. Fixed receipt predicates
reject inconsistent outcomes and wrong scalar types.

The two historical long resource trials retain their original strict spontaneous
criteria, observation windows and comparisons behind an explicit opt-in. Recovery
workload results are excluded from resource comparisons. No performance improvement,
upstream causal fix, native GUI or authenticated SSH claim follows from this change.

## Verification status

The host aggregate passed 1,036 Rust tests across 40 suites, with 29 opt-in tests
ignored on this host. All 14 new pure idle tests passed. Strict host and MSVC
all-target/all-feature Clippy passed; cross-checking does not execute Windows.
Independent acceptance review found no remaining blocker after correcting real-clock
admission, retained Client ownership when early reap is refused, and separate idle
timeout reporting.

Python suites passed: export 2, capabilities 5, Git fixture 6, Maven cache 11,
resource observer 79 and GC collector 36. Bundle tests passed 28/29 with one skip;
crash collector passed 80/85 with five platform/tool skips; Maven predicates passed
5/7 with two skips. PowerShell is unavailable on this host. Native CI must execute
the actual strict predicate matrix, including missing/array/wrong-type fields,
duplicates, contradictory recovery/cleanup and late-budget cases.

The all-feature release and default-feature frontend/agent release both built.
Exact native CI, whether conditional recovery actually ran, and a regenerated
package remain pending. A native spontaneous-only pass would not be
reported as observed recovery.


## Native predicate checkpoint and narrow repair

Public `0d30684d69d25d3b3f670d76b418a18ccc5d9913` ran
[CI 37912542153](https://github.com/LLLLimbo/cedar-ide/actions/runs/37912542153).
Both platforms executed the actual PowerShell matrix and failed because the
nested `array_receipt` negative was accepted. This early failure prevented Rust
compilation and Java/Maven execution; no package or runtime verdict resulted.

The repair preserves the received JSON array through the call site and object
parameter, then checks each indexed scalar receipt before matching. It removes
the filtering pipeline at that boundary. The original failing negative remains,
with additional scalar/null/deep-array/mixed-valid controls and an independent
argument-shape assertion in the native harness. Collector duplicate-key rejection
remains separate from PowerShell's parsed-object checks. Production/Rust code,
all timing criteria and historical failed verdicts are unchanged. The same 0.26.1
development version requires a new exact native run before acceptance.


## Exact repaired native result

Public `4c22638b35f6ad1abdede1576994080c3793a617` passed
[CI 37913418284](https://github.com/LLLLimbo/cedar-ide/actions/runs/37913418284)
on both platforms. The actual PowerShell adversarial matrix passed. Both platforms
again executed three merge cases with all thirteen witnesses. Required idle Java
matched spontaneously in 38,750 ms, with zero recovery attempts; conditional
recovery was not exercised. Primary/cleanup transition occurred at 37,774 ms,
all fixed budgets and cleanup evidence passed, and the retained root exited
naturally with code 0. Maven and all other required gates passed. The historical
long resource pair was not requested, and its earlier failed trial remains failed.

The verified unsigned Windows ZIP contained 533 payloads plus its manifest,
5,024,527 bytes, SHA256
`1a92b46d502fb617f54da7e60040cf4855738e9a408782cdabf65d7dc08acf43`.
Package verification does not establish Windows native GUI or authenticated SSH.
