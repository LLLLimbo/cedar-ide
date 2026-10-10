# Verification report · Linux desktop packaging fixture correction / 0.41.1

The [0.41 report](TEST_REPORT_PHASE41_LINUX_DESKTOP.md) preserves the fixed-sibling route, local semantic evidence, and the first Windows CI failure. The latest fully accepted checkpoint remains 0.40 until the new exact CI, three archives and native trust-off desktop gate complete.

## Narrow correction

The desktop packager still requires real Ubuntu 24.04 amd64 and regular 0755 executable inputs. Its production code is unchanged. Cross-platform tests now independently exercise dirty-source rejection, malformed ELF payloads for both pair members, and a valid-hash archive with a broken guide link. The real-filesystem build-path test explicitly requires POSIX modes and proves wrong mode rejects before malformed ELF, then proves valid mode reaches the ELF rejection. Its original dirty-source and broken-guide build assertions remain. Adjacent build/extraction tests requiring actual Unix permissions already carry explicit POSIX labels; pure archive and schema checks remain cross-platform.

This is a test-fixture portability correction, not a relaxation of package permissions or an inferred runtime defect. Cargo package versions advance together so the exact sibling-version handshake remains meaningful. README keeps the corrected release-profile launch command.

## Acceptance status

Final correction checks passed locally: formatting, strict host/MSVC all-target/all-feature Clippy, and the all-target aggregate (1,438 passed, 51 opt-ins ignored across 46 suites). Python ran 450 tests: 441 passed and nine platform/tool-dependent cases skipped; the desktop subset ran all 38 successfully on Linux. Exact dual-platform CI and all three artifact audits remain pending. The previous 0.41 local default binaries and typed runtime receipts are preserved; this test/version-only correction did not repeat those local runtime operations, and requires fresh default release and typed-route acceptance in CI. The Ubuntu-built desktop archive must subsequently pass the allowed generated-workspace, trust-off GUI workflow on cloud Debian 13 X11. This does not establish universal Linux/Wayland support, user-device validation, GUI trust-on or real SSH acceptance. Java/Maven runtime budgets and the 28-minute Linux Java orchestration envelope are unchanged.

## Exact CI outcome

Public `5b35f0691c63084b4264993eece03534d88e355b`, [run 38060735334](https://github.com/LLLLimbo/cedar-ide/actions/runs/38060735334), completed the corrected 38-test Windows packaging suite, with its POSIX-only tests explicitly skipped, then failed two acceptance-lifecycle mock assertions. The production driver canonicalized its owned temporary paths while expected fixture paths retained the Windows short-name alias. The failed run remains failed; independent Ubuntu evidence does not establish full acceptance.
