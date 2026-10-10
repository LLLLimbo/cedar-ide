# Verification report · Linux isolated-agent Maven leaf profile / 0.39.0

The [0.38 compatibility checkpoint](TEST_REPORT_PHASE38_CAPABILITY_GROUPS.md) is fully verified. This slice implements the existing strict Maven leaf subset on Linux isolated agents. The local generated pair passes; fresh Ubuntu/Windows CI and package verification remain pending.

## Contract

Linux retains 31 flat capability names and advertises the two exact versioned Maven groups. Windows retains its existing direct names and omitted group field. Protocol 4, metadata schema 1 and the 32-name flat limit stay unchanged. Linux Local/InProcess and validation/diagnostic profiles remain gated.

The native Java recipe uses config_linux, explicit absolute Java and existing owned Linux cleanup. ASCII control/data-home and Unicode workspace/distribution/cache paths retain strict path, leaf POM, environment, startup/POM hash and session constraints. Controls are created in a new app-owned subdirectory. There is no wrapper, goal invocation, automatic cache download or broader POM support. Offline dependency resolution is not a network sandbox; trusted configurator code and public Gradle metadata requests remain disclosed.

## Required acceptance

Normal release agent/Client present and missing cases use the same pinned JDT/JDK and frozen 83-file, 4,065,288-byte cache. Each case has 480 seconds: 360 primary plus 120 reserved cleanup. The pair has 960 seconds within a 1,020-second external watchdog. Existing 30/75-second Client calls require full admission; production deadlines do not change and insufficient remaining budget is a failure. The existing Windows pair remains unchanged.

Required witnesses include actual Maven nature/custom source/compiler 17, exact dependency identity/classpath/semantics, explicit unresolved model/full-GAV offline POM error with absent JAR/POM, bounded dependency insight, actual frontend metadata/snapshot/POM binding and dirty Undo preservation, restart-required POM changes, sealed input hashes and bounded resolver metadata. Linux Stop must distinguish code from signal and prove joined ownership, followed by client reaping. Forced cleanup is not graceful exit; missing proof is never accepted as clean.

## Local checkpoint

The initial implementation passed strict host and Windows MSVC checks, 1,409 Rust tests across 45 suites (49 opt-in tests ignored), and 368 Python tests (nine platform/tool skips). The first generated Linux pair remains failed: its present-dependency case passed the model, semantic, dependency-insight and frontend witnesses; its missing-dependency case established an unresolved model but failed during event validation. Both cases preserved input hashes and completed joined natural exit-code-zero cleanup. That receipt does not identify the rejected event and is not a passing pair.

The second, diagnostic-only run also remains failed. Its finite receipt identified the exact owned project marker without a terminal URI slash; code, severity, Java source, zero range, exact owned missing-library message and artifact-absence checks all matched. The fixture now accepts that one spelling only for the missing case. Shared URI comparison and production validation are unchanged. Missing success additionally requires both the full-GAV POM diagnostic and exact project marker; foreign, ambiguous and malformed URI cases remain rejected.

The repaired local pair passed in 28,322 ms on the cloud Debian host with the normal release agent and existing pinned JDK/JDT/cache. Present and missing cases each used one model query and established imported/present versus unresolved/absent observations, actual frontend binding and POM-restart guards. Both retained sealed inputs, six lifecycle metadata markers with mask 63 and zero foreign repository files, then completed graceful exit code 0, joined cleanup, client reaping and fixture removal. The agent hash stayed unchanged. The receipt explicitly records a dirty checkout and caller-supplied prebuilt agent; it does not claim final-source or Ubuntu package equivalence. CI must independently establish that linkage.

Final local checks passed: 1,414 Rust tests across 45 suites (49 opt-ins ignored), strict host and Windows MSVC checks, and 373 Python tests (nine platform/tool skips). The focused native fixture ran separately as described above. Fresh Ubuntu/Windows CI and both package audits remain pending. No SSH deployment, user-device operation, Linux Local Maven, network isolation, native Windows GUI or resource improvement claim is added.
