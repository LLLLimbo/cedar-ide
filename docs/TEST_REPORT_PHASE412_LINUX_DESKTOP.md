# Verification report · Canonical desktop acceptance fixtures / 0.41.2

The [0.41 report](TEST_REPORT_PHASE41_LINUX_DESKTOP.md) and [0.41.1 correction report](TEST_REPORT_PHASE411_DESKTOP_FIXTURE.md) preserve earlier local evidence and both failed Windows CI outcomes. The latest fully accepted checkpoint remains 0.40 until exact CI, all three archives and native trust-off desktop acceptance complete.

## Narrow correction

Only test fixture preparation changes: each new owned desktop fixture resolves its existing temporary root before creating children. Expected repository, binary and scratch paths remain exact; production path validation and canonicalization are unchanged. A platform-independent controlled alias test exercises success and failure, proves resolution precedes writes, retains a separate untouched sentinel, and creates no symlinks or privileged aliases. All new desktop mock path boundaries were reviewed: lifecycle inputs cross a canonicalization boundary; the subprocess supervisor passes its supplied cwd unchanged; payload and archive validators have no host-path equivalence assertion. The packaging fixture root is also canonical before source creation. Existing Java/Maven copied-harness fixtures already use canonical owned roots.

All negative packaging cases from 0.41.1 remain, including the native POSIX 0755 ordering checks and cross-platform malformed ELF/guide checks. Production Rust, packaging, acceptance supervision and CI are unchanged apart from the synchronized Cedar package version. Non-Cedar dependency lock entries remain identical.

## Acceptance status

Local Python ran 451 tests: 442 passed and nine existing platform/tool-dependent cases skipped. Formatting, locked offline Cargo metadata, structured dependency-lock equality and diff checks passed. The contained alias regression executed both success and failure cases. Independent static review found no remaining blocker. No new-version local Rust build or typed runtime is claimed for this test-only correction. Fresh versioned Rust builds, default-agent route/runtime checks and three artifact audits are required in exact CI; earlier local 0.41 binaries are not evidence for the new version. The final GUI gate must use the new verified Ubuntu-built archive on cloud Debian 13 X11 with trust off and generated files only. No universal Linux/Wayland, user-device, GUI trust-on, real SSH or resource-improvement claim is made.

## Final exact acceptance

Public source `b8c6f540aa0b516cfe5ba986e816dfccfde66430` passed [both CI jobs](https://github.com/LLLLimbo/cedar-ide/actions/runs/38061337370). All three archives were independently verified against source, modes, hashes and measured ABI. The exact Ubuntu-built desktop archive also passed a cloud Debian 13 X11 trust-off GUI check using generated files: editing, explicit Save, Undo/Redo, retained draft across Disconnect/reconnect, and normal window close. All 527 extracted file hashes/modes remained unchanged. The Maven checkbox was not inspected, no GUI Java/Maven execution occurred, and native agent reaping was not independently observed; headless ownership evidence is separate. Windows ran 38 packaging tests (30 passed, eight explicit POSIX skips) and all 24 lifecycle tests. Earlier failed runs and the rejected 0755 recovery fixture remain historical evidence.
