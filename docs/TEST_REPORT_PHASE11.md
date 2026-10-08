# Verification report · connection cancellation / 0.11.0 · 2026-10-08

This checkpoint adds bounded cancellation of process connection/read waits and
observational process-tree resource evidence. Local verification passed; exact native CI for this checkpoint is pending.

## Verified public baseline

Exact public 0.10.0 commit
[`77cb3b8d50c30f6daf2ba4276822e9f7416c4dac`](https://github.com/LLLLimbo/cedar-ide/commit/77cb3b8d50c30f6daf2ba4276822e9f7416c4dac)
[passed Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37736569381).
All nine interrupted-save process cases actually executed on each platform with
zero ignored cases. The prior direct Java, agent/editor, task independence,
forced-owner and normal production Java acceptance also passed.

Production Java Stop remained honestly forced with grace_expired and root exit
1067, completed shutdown protocol, joined cleanup and verified client reap.
That is not a natural production-exit claim. The independent strict direct and
fixture sessions retain their own natural-exit requirements.

## Changes and limits

[Connection cancellation](CONNECTION_CANCELLATION.md) uses a permanent per-session
token and 50 ms maximum receive slices only while process Hello/List/Read/Search
is pending. Original absolute deadlines remain unchanged. Already process-enqueued
mutations retain their normal replies; old app-queue work is discarded. No writes
or commands are replayed. Existing save, language and task reconnect guards remain.

Cancellation completion, direct-child reaping and resource-count settling are
separate assertions. The change does not join detached pipe threads or establish
arbitrary descendant cleanup. No idle-connection polling or additional transport
thread is added. Embedded synchronous workspace calls and OS process creation
remain outside the interruption guarantee.

The resource baseline is observational. It cannot establish an IntelliJ IDEA
comparison, a complete GUI footprint, authenticated SSH interoperability or fully
settled project indexing. Native GUI execution-trust and real SSH testing remain
separate uncompleted validation.

The sealed 0.10.0 behavior and local verification record is retained in
[the preceding report](TEST_REPORT_PHASE10.md); later CI outcomes are stated above
rather than retroactively changing that report.

## Local verification

Rust 1.99.0 host aggregate: 637 passed, zero failed. Explicit acceptance added
three new app process cases, nine interrupted-save cases and seven normal-agent
process cases: 656 executed Rust cases in total. Five Python agent protocol/tool
smoke chains passed. Strict whole-workspace host and MSVC cross-target clippy,
formatting and the optimized workspace build passed. The export regressions
passed two tests; the existing Java evidence sanitizer passed 62 with two
native-Windows skips. The resource observer passed 18 synthetic/accounting/privacy
tests, including a generated native Linux Python process tree. Its actual Windows
backend and real Java measurements remain pending native CI.

The client subset separately passed 44 unit tests, eight public cancellation
cases and one isolated resource-count regression. After warmup, Linux descriptor
and thread counts were 4 and 3 at baseline and after each eight-cycle batch.
This is a debug-test cleanup observation, not Cedar product-memory evidence.
MSVC cross-checking is not native Windows execution. Exact commit CI must run
the new cancellation cases and obtain the real agent/JVM observation.
