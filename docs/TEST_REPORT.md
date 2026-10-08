# Verification report · fixed GC diagnostic control / 0.13.0 · 2026-10-08

This checkpoint adds one opt-in nonshipping diagnostic control, retaining the
production 512 MiB heap recipe and collector selection. Local verification passed; the exact native diagnostic run is pending. The control records GC-point numeric evidence;
it does not tune the product or infer unused heap from resident memory.

## Verified baseline

Exact public 0.12.0 commit
[`69044092d4c4f8532cad4ff2513f770af716afeb`](https://github.com/LLLLimbo/cedar-ide/commit/69044092d4c4f8532cad4ff2513f770af716afeb)
[passed Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37749233834).
Both ordinary long trials passed all semantics/editor/source and cleanup checks,
with graceful root exit 0 and verified client reap. All eight stage latencies
were present in each. The native observer suite passed 62 tests with one platform
skip; previous cancellation/save/task/Java gates remained green.

All four final windows had 50 stable RSS samples, about 9.82 seconds of observed
coverage, no unknown RSS samples and documented gaps within the 400 ms allowance.
Tree working-set medians were 791.36/778.96 MiB after initial diagnostics and
840.30/819.04 MiB after correction. Independent-window CPU estimates were
2.865/4.615% and then 0.159/0.318% of one logical core. This is a debug headless
driver/release agent/JVM observation, not a GUI footprint, settled-state guarantee
or IDEA comparison. Low observed CPU plus retained resident memory does not
establish unused committed Java heap.

## Diagnostic boundary

The [fixed control](GC_DIAGNOSTIC_CONTROL.md) uses a separate constructor/binary
and distinct receipt, with exact synthetic opt-in markers and ordinary execution
trust. Normal constructors and shipping CLI remain isolated under all features.
The shared production launch recipe file is unchanged; only the diagnostic path
adds the fixed private file-logging option. Logging overhead is explicit.

The sampler verifies the retained JVM identity. Its exact private-witness digest
binds the numeric collector's selected log reads before and after collection.
The general crash scan excludes private GC names before even reporting rejected
metadata. Only bounded numeric/enumerated data and digests are published.
Natural zero exit and full semantic cleanup are required independently of log
observations; partial observations remain partial and trigger no tuning.

The preceding local report is preserved in [the long baseline report](TEST_REPORT_PHASE12.md).

## Local verification

Rust 1.99.0 passed 649 aggregate tests plus 19 explicitly executed process
acceptance cases (668 total), five Python agent smoke chains, formatting, strict
whole-workspace host/MSVC checks and the optimized build. The default-feature
Windows shipping app/agent cross-check also passed. The focused diagnostic-host
subset passed nine workspace and three CLI tests; Windows-only controls remain
for native execution. The shared production java_launch.rs is byte-identical.

The GC collector passed 36 tests; the sampler passed 79; the crash collector
passed 69 with two native-only skips; export tests passed two. Independent review
confirmed digest handoff binding, enclosing-scan filename privacy, fixed-default
isolation and the partial-observation limits. Exact-commit native Windows must
still execute the one diagnostic control and verify its matched identity,
semantic/lifecycle receipt and actual numeric GC output. No local Java control
was run.
