# Verification report · Windows development bundle / 0.14.1 · 2026-10-08

This checkpoint adds a versioned unsigned Windows development ZIP, exact source
and workflow mapping, payload hashes, Chinese quick-start and native extracted
bundle verification. It uses the normal default-feature release app and agent.
The package excludes diagnostic binaries, language runtimes and raw test logs.
Version 0.14.0 passed exact native bundle execution. Version 0.14.1 changes only
version metadata and documentation to include the verified VC++ runtime prerequisite;
its regenerated artifact and exact native CI are pending.

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

At 0.14.0, the unchanged Rust code passed 649 aggregate tests plus 19 explicitly executed process cases with Rust 1.99.0
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


## Native 0.14.0 bundle and prerequisite correction

Exact public commit
[`76f7ffcd82a7a219116506c0899cba6a9e31b831`](https://github.com/LLLLimbo/cedar-ide/commit/76f7ffcd82a7a219116506c0899cba6a9e31b831)
[passed both CI jobs](https://github.com/LLLLimbo/cedar-ide/actions/runs/37762972011).
The Windows package suite ran 29 cases: 28 passed and one platform-specific case was skipped. The default-feature
release rebuild and extracted Unicode/space-path probe passed every trust-off
file, conflict, execution-rejection and reaping assertion. Package bytes remained
unchanged and the synthetic root was removed. Required Java suites remained green.

The uploaded artifact digest and all 524 inner payload hashes were verified,
with exact source/run mapping. The 4,563,984-byte inner ZIP SHA256 is
`121e3c00b1c6a7a33d8f042dca9ec5388136350a2de806aa7d56e41553743c7f`.
Read-only PE import inspection found `VCRUNTIME140.dll` and UCRT API sets in both
executables. The first quick-start omitted this prerequisite. Version 0.14.1
corrects that documentation and links Microsoft's x64 runtime; it does not change
Rust behavior, redistribute runtime DLLs, install software or alter the old ZIP.
The corrected version will regenerate its own source/hash manifest through CI.


For the 0.14.1 documentation/version correction, local package regressions again
ran 29 cases (28 passed, one Windows-only skip), export regressions passed two,
and the final diff check passed. Rust source and packaging/probe implementation
are byte-identical to 0.14.0. Full native CI will rebuild the versioned binaries
and verify the corrected guide's new payload hash before delivery.
