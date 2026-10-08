# Verification report · longer Java observation / 0.12.0 · 2026-10-08

This checkpoint adds exactly two fixed longer observation trials of the existing
512 MiB normal production Java recipe. Local verification passed; exact native
CI for the two longer trials is pending. No production heap, collector, protocol or trust setting changes.

## Verified baseline

Public 0.11.1 commit
[`7b00948868417b13b2aeb3a8b67a056f4c0b81de`](https://github.com/LLLLimbo/cedar-ide/commit/7b00948868417b13b2aeb3a8b67a056f4c0b81de)
[passed exact Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37744067995).
The native observer suite passed 33 cases with one platform skip. All previous
cancellation, interrupted-save, Java/editor, task and ownership gates passed.

The schema-2 artifact verified 429 positive CPU intervals with consistent
midpoint and timing bounds, QPC resolution of 100 ns, and CPU read spans from
5.5 microseconds to 1.27 milliseconds. CPU is explicitly estimated per process;
its sum is not one exact shared-window tree measurement. No values were clipped.
The observed summed working-set peak was 849.18 MiB for the debug headless driver,
release agent, JVM and observed descendants. That differs from the earlier
brief run but establishes no optimization or regression. All four observed
instances exited. Production Java Stop remained honestly forced after its grace
expired; this is not a natural-exit claim.

## New experiment

The [longer baseline](LONG_JAVA_BASELINE.md) retains all normal-route semantic,
source and owned-cleanup assertions. Each of two separate runs adds fixed
30-second observations after initial and corrected diagnostics, eight typed
interaction latencies and final-ten-second sampled-window summaries. It does
not extend the existing watchdogs or wait until an apparent idle state occurs.

Each trial must independently produce exactly one complete sanitized production
receipt. The observation report can explicitly remain incomplete; CPU or memory
values are not product pass thresholds. Comparison checks same-version inputs
and reports two observations, not a tuning or statistical percentile claim.

The preceding local report is retained in [the CPU repair report](TEST_REPORT_PHASE11_CPU.md).

## Local verification

Rust 1.99.0 passed 637 aggregate cases and 19 explicitly executed process
acceptance cases (656 total), five Python agent smoke chains, strict host and
MSVC cross-target workspace checks, formatting and the optimized build. The
observer/comparison suite passed 63 cases, including actual native Python
executable fingerprinting, sampled-window edge/gap handling, elapsed-weighted
CPU integration, bounded input hashing, hostile metadata and failure-code
preservation. Export tests passed two; the existing Java evidence collector
passed 62 with two native-only skips.

A Windows-specific fingerprint issue was corrected before publication: CPython
path stat can infer executable permission bits while handle fstat does not.
Checks compare regular-file type and stable device/inode/size/mtime. Because
Windows path-stat ctime can mean birthtime while handle fstat returns ChangeTime,
ctime stability is checked separately within each API. Identity and change
protection are retained rather than discarded to accommodate the differences.
The actual Windows executable witness and both real Java long trials still
require the exact native CI run. No new local Java experiment was performed.
