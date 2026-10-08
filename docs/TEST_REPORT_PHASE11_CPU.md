# Verification report · CPU observation timing / 0.11.1 · 2026-10-08

This checkpoint repairs the observational CPU timing introduced in 0.11.0.
It does not change the Java launch recipe, heap ceiling, semantic workload,
shutdown policy or connection cancellation behavior. Exact native CI is pending.

## Verified public baseline

Exact public 0.11.0 commit
[`c257571ed3a39e3683ae3aeec535d4bcf457e9dc`](https://github.com/LLLLimbo/cedar-ide/commit/c257571ed3a39e3683ae3aeec535d4bcf457e9dc)
[passed Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37741303276).
Each platform actually ran eight public client cancellation cases, one isolated
resource-count regression, three app process cancellation cases and nine
interrupted-save cases. All prior Java, editor, task and owned-cleanup gates passed.
The native Windows observer test suite passed 17 cases with one Linux-only skip.

The sanitized process-tree artifact had 107 samples, no reported observation
issues, and a successful driver exit. Its maximum same-sweep summed working set
was 825.99 MiB, including 802.79 MiB for the JVM at that peak. The defined two-second
post-diagnostics interval had an observed maximum of 652.77 MiB. These are
resident working sets of a debug headless test driver, release agent, JVM and
observed descendants; shared pages may be double-counted. They are not a release
GUI footprint, settled-idle guarantee or IntelliJ IDEA comparison.

Production Java Stop remained accurately forced with grace_expired and root
exit 1067, completed protocol witnesses, joined cleanup and verified client reap.
This is not a natural production-exit claim.

## CPU timing limitation and repair

The old report contained a 487.45% JVM CPU estimate on a four-logical-CPU host.
That sample used a 234 ms interval between sweep starts, although the current
sweep took 78 ms. Per-process CPU counters were queried later in each sweep,
so that denominator did not represent the counters' own observation interval.
CPython 3.12 on Windows also used a coarse clock for monotonic_ns.
The old CPU peak is not evidence of JVM capacity above four CPUs.

The repair uses high-resolution per-counter observation brackets, explicit
timing uncertainty and clearly labeled sums of process estimates. It does not
clip CPU readings to an assumed machine capacity. Memory accounting remains a
same-sweep sum with the existing identity and missing-observation checks.
Details and interpretation are in [the resource baseline](RESOURCE_BASELINE.md).

The same two-second workload is retained for the corrected native measurement.
Longer idle and memory/latency experiments are separate future work; this
checkpoint makes no memory-optimization or heap-tuning claim.

The preceding sealed local report is retained in
[the 0.11.0 report](TEST_REPORT_PHASE11.md). Its later native result is stated
above rather than rewriting the historical report.

## Local verification

The final version passed 637 aggregate Rust tests plus 19 explicit process
acceptance cases (656 total), five Python agent smoke chains, strict host and
MSVC cross-target workspace checks, formatting and the optimized build.
The repaired observer passed 34 tests, independently rerun by review: native
Linux child-tree sampling, simulated Windows counter placement, delayed sweeps,
preemption, uncertainty, invalid timing, phase boundaries, privacy and failure
status. Export tests passed two; the Java evidence collector passed 62 with two
native-only skips. Cross-checking and simulation do not replace the pending
exact-commit Windows resource observation.
