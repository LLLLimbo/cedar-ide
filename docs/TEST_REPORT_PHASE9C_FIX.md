# Verification report · checkpoint 9C setup fix / 0.8.5 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

This targeted change corrects the Windows Java test setup. Production Rust
transport, Java semantic/Unicode assertions, native fixture gates and ordinary
Windows IDE capability/trust policy are unchanged.

## Actual 0.8.4 result

Public commit
[`25b86f49f11c82948a475d2591687b6080b3622e`](https://github.com/LLLLimbo/cedar-ide/commit/25b86f49f11c82948a475d2591687b6080b3622e)
passed Ubuntu CI. In the [Windows job](https://github.com/LLLLimbo/cedar-ide/actions/runs/37694454153/job/113042543464),
the prior 21 process, 12 language-transport and seven isolated-agent/bundle cases
passed. Real Java setup verified the exact JDT archive digest, then native tar
could not open its Unicode archive path. JDT was never started. The later three
agent-language cases were skipped; neither gate is counted as passed.

## Narrow setup correction

Only the archive/extraction staging path is ASCII. After successful extraction,
PowerShell moves the complete distribution to a Unicode/spaces runtime directory.
It verifies the old staging directory is gone, the new directory/config_win exist,
there is exactly one launcher, and the launcher filename/digest are unchanged.
All Java runtime, source and data-path Unicode checks remain. The pinned archive
SHA-256 is still checked before any extraction or execution. No machine code-page
or locale setting, new dependency or fallback to an unverified archive is added.

The same native log showed empty JDK_JAVA_OPTIONS, JAVA_TOOL_OPTIONS and
_JAVA_OPTIONS. [PowerShell's official documentation](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_environment_variables?view=powershell-7.5)
explains that SetEnvironmentVariable with null can preserve an empty value in
7.5. The Rust probe requires absence, so the setup now uses provider removal and
explicit absence checks. This prevents a second known setup mismatch; it does
not weaken the probe to accept inherited values. Example instructions were
corrected too. Changes affect only the isolated test parent's process environment.

## Checks and boundaries

- Whole-workspace aggregate, strict MSVC cross-target clippy and Linux release
  checks were rerun for 0.8.5. The aggregate has 550 passing Rust executions,
  two export regressions and five Python protocol chains, with zero failures.
- Independent source review checked the scoped staging/move and environment
  changes. PowerShell/native extraction must still be executed by Windows CI.
- The existing real Linux three-session Java results remain in the 9C report;
  this setup-only fix does not substitute another Linux run for Windows evidence.

Raw local logs: `verification-phase9c-fix-aggregate.txt`,
`verification-phase9c-fix-windows.txt`, `verification-phase9c-fix-release.txt`.
These machine-specific files are omitted from public source export.

A new exact-commit native run must complete real Java and all three nonshipping
agent-language cases, alongside prior regressions. Normal Windows IDE language
services remain disabled. Real JDT through the agent/editor, broader process/
listener observations, native GUI and authenticated SSH are still separate gates.
