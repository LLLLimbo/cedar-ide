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
