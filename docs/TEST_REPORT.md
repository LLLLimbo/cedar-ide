# Verification report · Windows environment lookup contract / 0.15.1 · 2026-10-08

This checkpoint corrects an unsupported Unicode assumption in a Windows-only
Git environment unit test and adds an independent native lookup gate. Production
Git filtering and owned process-launch behavior are unchanged. The Windows Git
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
The actual native relationship remains for this checkpoint's CI to establish.

## Verification status

Local Python syntax/help and non-Windows fail-closed checks passed; 94 focused
workspace tests, strict workspace/winprocess MSVC checks and formatting passed.
Package regressions passed 28 with one Windows-only skip; export passed two.
Independent static review found no remaining blocker in scope. Full exact-commit native CI must pass the new lookup gate,
existing environment units and all 25 lifecycle cases, real Git acceptance,
required Java/ownership/file suites, and regenerated default-feature Windows
bundle before a Windows feature or delivery claim. GUI and authenticated SSH
acceptance remain separate open work.
