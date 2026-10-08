# Verification report · Windows environment lookup contract and Git selection / 0.15.2 · 2026-10-08

This checkpoint selects exactly one native Git application in the Windows CI
driver. It retains 0.15.1's corrected Unicode unit and independent native lookup
gate. Production Git filtering and owned process-launch behavior are unchanged. The Windows Git
route remains pending exact native acceptance; the previous green user-download
baseline is 0.14.1, not the failed 0.15.0 Windows build.

## Exact 0.15.0 result

Public commit
[`3aee8ead3cb4ab9cc1fe326287a6998d72003431`](https://github.com/LLLLimbo/cedar-ide/commit/3aee8ead3cb4ab9cc1fe326287a6998d72003431)
[completed with Ubuntu success and Windows failure](https://github.com/LLLLimbo/cedar-ide/actions/runs/37774360771).
Ubuntu's actual normal-agent/real-Git probe passed 1,276 assertions, including
unchanged repositories, missing-promisor failure, output/deadline/root-exit helper
cleanup and independent task ownership. Windows failed the workspace unit that
assumed dotless-i spellings always compare equal to ASCII GIT_. The process
lifecycle, actual Git and updated bundle stages did not execute after that failure.
The earlier local implementation report is retained in [phase15](TEST_REPORT_PHASE15.md).

The failure establishes that the hardcoded removal expectation was unsupported
on that runner. It does not by itself determine whether native environment lookup
and ordinal comparison agree, or prove the actual Git route safe or unsafe.

## Required independent checks

An early Windows-only Python probe launches a separate isolated child for each
fixed case, with only SystemRoot and one synthetic key/value. It verifies the
original spelling through GetEnvironmentStringsW, exact/canonical lookup through
GetEnvironmentVariableW, and full-name CompareStringOrdinal results. ASCII and
Unicode controls are included. Only case IDs, result/error integers and booleans
are printed; raw environment strings and traceback data are excluded. Each child
has a ten-second deadline, no child spawning, and joined subprocess ownership.
API errors, changed spelling/value, failed controls or lookup/comparison
disagreement fail the gate. Parent environment mutation is checked separately.

The Rust unit now asserts removal or exact retention according to the measured
OS comparison, while ASCII Git-name removal remains mandatory. The actual Git
smoke keeps a separate ASCII redirect/trace attack pass and a Unicode-candidate
pass; it cannot pass merely because a candidate happens to be a distinct name.
Both the OS probe and actual Git safety checks are required. No production guard
is removed or replaced by a successful comparison test alone.

Microsoft recommends ordinal comparison for environment names but does not
promise the asserted dotless-i equivalence: [official guidance](https://learn.microsoft.com/en-us/windows/win32/intl/handling-sorting-in-your-applications).
The observed relationship from 0.15.1 is recorded below; the gate remains required on later CI runs.

## Verification status

At 0.15.1, local Python syntax/help and non-Windows fail-closed checks passed; 94 focused
workspace tests, strict workspace/winprocess MSVC checks and formatting passed.
Package regressions passed 28 with one Windows-only skip; export passed two.
Independent static review found no remaining blocker in scope. Full exact-commit native CI must pass the new lookup gate,
existing environment units and all 25 lifecycle cases, real Git acceptance,
required Java/ownership/file suites, and regenerated default-feature Windows
bundle before a Windows feature or delivery claim. GUI and authenticated SSH
acceptance remain separate open work.


## Native 0.15.1 result and 0.15.2 correction

Exact public commit
[`2b7622ffd5174598f6135a46956184db781fcd89`](https://github.com/LLLLimbo/cedar-ide/commit/2b7622ffd5174598f6135a46956184db781fcd89)
[passed Ubuntu; Windows reached the Git invocation step](https://github.com/LLLLimbo/cedar-ide/actions/runs/37778174600).
The seven-case native lookup probe passed. Exact ASCII, mixed ASCII and the
Greek case control compared equal and returned the synthetic value. Dotless-i
candidates and the unrelated-name control compared distinct and returned
ERROR_ENVVAR_NOT_FOUND (203). Raw spelling and exact-key lookup were verified in
every child; parent environment was unchanged. On this runner, the earlier
failure was an incorrect test/comment assumption, not evidence requiring a
production filtering change. This is a finite observed matrix, not a claim about
every possible Unicode name.

Windows aggregate tests, all 25 process lifecycle tests, required Java suites
and the default-feature shipping rebuild passed. The real-Git smoke did not
start: PowerShell application discovery returned multiple git.exe entries, which
expanded into extra argparse arguments. The bundle step was consequently skipped.
There is still no Windows real-Git or 0.15 bundle acceptance claim.

Version 0.15.2 changes only CI selection plus version/documentation metadata:
choose one ApplicationInfo result, validate a scalar existing absolute path, and
pass it as one argument. Production Rust and probe implementation are unchanged.
The actual-Git ASCII/Unicode redirect tests, ownership checks, lookup gate and
all previous required suites remain required on the new exact commit.


For the 0.15.2 selection/documentation change, local package regressions again
passed 28 with one native-only skip, export regressions passed two and diff checks
passed. Rust and Python probe source are byte-identical to 0.15.1; native CI will
perform the full rebuild and actual Git invocation before a delivery claim.
