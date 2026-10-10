# Verification report · Linux GUI fixed sibling agent and desktop bundle / 0.41.0

The previous 0.40 checkpoint passed exact-source dual-platform CI and package audits: public `3b821523a1c82bb8db7266cce5d2b7d6adb70070`, [run 38056621659](https://github.com/LLLLimbo/cedar-ide/actions/runs/38056621659). Its [preflight report](TEST_REPORT_PHASE40_SSH_PREFLIGHT.md) records the bounded change.

## Contract

Linux GUI Local selects only the normal cedar-agent beside its own executable. The public Client Local route remains embedded; Windows and SSH policy are unchanged. Local recovery identity is unchanged and restoration remains trust-off. The bundled route checks regular nonsymlink native executable permissions and rejects missing or incompatible files without PATH search, shell, automatic repair or fallback. It requires valid protocol/metadata, Linux/native architecture, exact package version and canonical UTF-8 workspace root before Ready. Version and file checks are compatibility checks, not authentication or protection against same-user replacement.

The same owner is retained through Hello failures and cancellation. Post-spawn failure cleanup observes the existing reaper for three seconds with its two-second grace; OS spawn and kernel cleanup are not hard-realtime. Verified, unverified and no-child failures remain distinct. An owner-created uncertainty notice preserves one bounded warning lineage even after an attempt is cancelled or superseded, without adopting old workspace state. No peer/error text can create that notice. Existing successful-client cleanup and language Stop semantics remain separate.

## Packaging and acceptance

A distinct Ubuntu 24.04 amd64 development archive contains the exact default frontend and agent, source-linked hashes, modes, licenses and guides. Each ELF has a separate measured ABI record; the GUI may additionally require libm. Dynamically loaded display/GL libraries and system CJK fonts require independent runtime evidence; the archive is not universally portable and installs nothing. The agent-only archive and Windows ZIP remain required outputs.

Generated acceptance copies a nonshipping harness/probe beside the exact normal agent and calls the same production sibling route; no arbitrary selector ships. Existing pinned Java and Maven workloads retain their fixed budgets and source/cleanup witnesses. The Maven connect reservation admits the existing 30-second Hello plus bounded failure cleanup within its unchanged 75-second call envelope. Typed features still require execution trust and explicit configuration; offline Maven dependency resolution is not network isolation.

Native cloud GUI checks are limited to trust-off generated files, editing/save/Undo and disconnect/reconnect. No user device, GUI Trust-on, deployment, SSH, font installation or network policy changes are included. The local checks below are complete. Actual display-backend observations, exact CI and all three archive audits remain pending; GUI acceptance will use the verified Ubuntu-built archive on the cloud Debian 13 X11 desktop, with that runtime-host distinction retained.

The copied Linux app harness additionally runs the shared Run/save/cancel workflow through GUI Local’s fixed-sibling connection: one exact selected case, a separate 60-second supervisor and 15-second selection bound, and explicit verified disconnect. The original aggregate case remains an explicit embedded-Client test with the same assertions. Java/Maven runtime budgets are unchanged; the Linux Java CI orchestration envelope is 28 minutes: 1,625 seconds of declared subprocess budgets including provenance and JDK checks, with no Java or Maven request/window extension.

## Local final-code evidence

On the cloud Debian 13 x86_64 executor, strict host and MSVC all-target/all-feature Clippy and formatting passed. The aggregate passed 1,419 tests with 51 explicit opt-ins ignored across 52 suites; all 11 sibling-client tests, the controlled 20-case sibling rejection integration, and worker cancellation/panic/late-warning tests executed. Python ran 447 tests: 438 passed and nine platform/tool-dependent cases skipped. Those skips require their native CI outcomes. The previous aggregate failure in the old GUI-Local-assumed-embedded Run fixture is retained; its explicit embedded replacement and mandatory copied-sibling counterpart preserve the shared assertions.

The default frontend and agent built successfully. With the pinned existing JDT 1.61 archive and unchanged JDK/cache inputs, the copied-sibling Java workload passed in 22,301 ms and its separate Run/save/cancel case executed once with verified disconnect. Correction was spontaneous with zero refresh recovery. Initial Java Stop was graceful exit code 0; restart Stop was forced after grace expiration, signal 9, with joined cleanup. The Maven present/missing pair passed in 30,387 ms with exact model, dependency, marker, source preservation and cleanup predicates. Both workloads verified the original and copied normal-agent hashes. These local receipts describe caller-supplied prebuilt binaries and a dirty source snapshot; final source/package equivalence still requires the exact CI build and archive audit.

No Linux graphical trust-on, Windows GUI, actual SSH deployment, universal Linux portability or resource improvement claim follows from these tests. Native trust-off desktop acceptance remains a separate required completion step.

## First exact CI outcome

Public `bf89cc153f3e26ee379aa19b3854a81be103a271`, [run 38060003148](https://github.com/LLLLimbo/cedar-ide/actions/runs/38060003148), failed its early Windows desktop-packager Python test. The intended malformed-ELF fixture encountered the strict regular-0755 check first, because Windows does not represent the synthetic Linux execute modes. The failed run remains failed; Ubuntu results are independent. No full desktop readiness claim was made.
