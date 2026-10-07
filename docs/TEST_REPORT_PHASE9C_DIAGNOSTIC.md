# Verification report · checkpoint 9C JVM diagnosis / 0.8.7 · 2026-10-07

This checkpoint adds bounded crash diagnosis. It does not claim to repair the
Windows JVM failure or enable normal Windows language capability.

## Observed native result

The exact public 0.8.6 commit
[`68062d4b1d6f634d96a1be6aef55448c073f3adc`](https://github.com/LLLLimbo/cedar-ide/commit/68062d4b1d6f634d96a1be6aef55448c073f3adc)
passed Ubuntu and the existing 21 process, 12 language transport and seven
isolated-agent/bundle Windows cases. Its
[Windows run](https://github.com/LLLLimbo/cedar-ide/actions/runs/37701128755/job/113064565672)
then failed before Java initialization: root exit 1, incomplete LSP header and
stderr reporting MiniDumpWriteDump error 0x80070006. The previous jarfile-open
error did not recur. Source bytes remained unchanged. No Java semantic assertion
passed, and the later three agent-language cases were skipped.

The primary JVM fatal reason is unknown. HotSpot emits fatal report text on
stdout, which is not an LSP frame; a partial-header error alone does not establish
a Cedar decoder bug. Upstream Windows crash-dump code also uses a narrow current
directory/file path and can pass an invalid dump handle after file creation
fails. The observed MiniDumpWriteDump error may therefore be secondary. No Job,
handle inheritance or transport guarantees are relaxed on this hypothesis.

## Diagnostic additions

- A nonshipping Windows example runs six bounded owned-process cases: ordinary
  and canonical Java paths with ASCII and Unicode cwd for `-version`, then each
  executable spelling with the same known Java stdin/stdout/stderr source probe
  under Unicode cwd. The source probe uses the relevant JDT JVM options.
- It records full root exit status, bounded output byte counts, binary stdin
  acceptance and expected stream markers, cancellation/drain completion and
  owned Job zero. Each case has a 20-second execution and five-second cleanup
  budget. Failed children do not prevent later diagnostic cases from running.
- Both diagnostics and the real JDT probe select an ASCII-owned ErrorFile path
  and suppress core dumps. Actual Java acceptance assertions remain unchanged.
- Raw VM output, including fallback fatal reports, stays in generated private
  scratch. It is never streamed to CI console or copied into public artifacts.
  The collector reconstructs only allowed fatal categories/frames and typed
  lifecycle/acceptance metadata, with bounded reads, source hashes, explicit
  partial/error states and no environment, register or memory dumps.
- Collection precedes scratch removal on success or failure. Missing fatal logs
  are not proof that no crash occurred. Driver completion and evidence collection
  are not acceptance; the real Java process and assertions remain the CI gate.

## Verification and remaining boundary

Local aggregate checks passed 555 Rust tests, including six explicitly invoked Linux
stdio/lifecycle cases; all five Python agent/profile smoke chains passed. MSVC all-target,
all-feature clippy and optimized all-feature workspace build passed. The final sanitizer suite passed 35 tests; its native Windows junction test
was skipped on Linux and remains required in Windows CI. Independent privacy
review found no remaining raw-output disclosure or schema blocker. The real
Linux JDT regression with the new ErrorFile flags passed all three sessions in
27,009 ms; sanitizing its actual private transcript retained all session/source/
cleanup witnesses with complete collection status and no raw payload. This is
regression evidence, not a performance benchmark or Windows result. Native Windows
execution is still required to learn the primary fatal reason and validate the
new diagnostic matrix. No Java/agent/editor/GUI/SSH completion is inferred from
cross-compilation. The ordinary Windows capability gate and trust checks remain
unchanged; GUI Trust and authenticated SSH approvals are still absent.

Sources: [HotSpot fatal report output](https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/utilities/vmError.cpp),
[Windows crash dump implementation](https://github.com/openjdk/jdk21u/blob/master/src/hotspot/os/windows/os_windows.cpp).
