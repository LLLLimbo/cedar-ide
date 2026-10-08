# Verification report · Windows development bundle / 0.14.0 · 2026-10-08

This checkpoint adds a versioned unsigned Windows development ZIP, exact source
and workflow mapping, payload hashes, Chinese quick-start and native extracted
bundle verification. It uses the normal default-feature release app and agent.
The package excludes diagnostic binaries, language runtimes and raw test logs.
Local checks passed; exact native bundle execution remains pending for this checkpoint.

## Verified baseline

Exact public 0.13.0 commit
[`e93172385bed14855f62edf86ae8253c05251dcc`](https://github.com/LLLLimbo/cedar-ide/commit/e93172385bed14855f62edf86ae8253c05251dcc)
[passed Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37758115385).
The single fixed GC control passed full semantics/editor checks, natural root
exit 0 and client reap, with the shipping agent hash unchanged. Numeric evidence
was complete, matched the sampler-selected JVM, and identified G1 with 54 pause
events. Last/max post-GC occupancy was 378 MiB and capacity 512 MiB; the last event
at uptime 37.390 seconds was not a final-idle live-heap measurement. No production
heap/collector change follows. Previous native Java, task ownership, cancellation
and interrupted-save suites remained green.

The diagnostic question is answered. Its optional invocation leaves routine CI;
the isolated fixture and unit tests remain. Required Java correctness suites are
retained. See [the diagnostic result and limits](GC_DIAGNOSTIC_CONTROL.md) and the
[preceding local report](TEST_REPORT_PHASE13.md).

## Bundle acceptance scope

The [bundle workflow](WINDOWS_BUNDLE.md) checks an explicit allowlist, bounded
stable file reads, valid x86-64 PE binaries, complete hash inventory and safe
archive names before extracting into a new Unicode/space path. The nonshipping
probe uses normal Local sibling-agent discovery with trust off. It exercises
listing, reading, conditional save, readback, search, stale-write refusal,
execution rejection and owned child reaping. Synthetic saved bytes, unchanged
package payloads and scratch removal are checked independently by the wrapper.

This is headless validation of the delivered file route. Windows GUI interaction
and authenticated SSH remain unverified; the ZIP is not a signed installer or a
production-readiness claim. Existing Java stop reporting still distinguishes
forced cleanup from natural exit. The included resource notes describe earlier
headless observations, with no claim of GUI footprint or superiority to IDEA.

## Local verification

Rust 1.99.0 passed 649 aggregate tests plus 19 explicitly executed process cases
(668 total), five real-agent Python chains, formatting, strict whole-workspace
host/MSVC checks, default-feature shipping MSVC checks and the optimized normal
app/agent build. The package regression suite passed 28 tests with one native
Windows junction case skipped locally. Existing Python suites passed two export,
69 crash-collector tests with two Windows-only skips, 79 sampler and 36 GC tests.

Independent review reproduced and verified fixes for Windows pathname/handle
metadata differences and hidden trailing DEFLATE bytes. All 517 existing license
notice paths match the package allowlist. PowerShell and the actual x64 PE bundle
probe require native Windows CI; Linux unit fixtures are not runtime evidence.
The final delivery must additionally verify the exact uploaded artifact digest,
inner ZIP manifest, source/CI mapping and successful native probe receipt.
