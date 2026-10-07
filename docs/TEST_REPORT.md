# Recovery hotfix verification · 0.3.1 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

This minimal checkpoint fixes two recovery-storage defects found after phase 3.
It adds no language features and keeps workspace protocol 3 and recovery format 1.
The original phase-3 report is preserved in `TEST_REPORT_PHASE3.md`; it records
what passed then and does not negate the later failures described here.

## Why this hotfix exists

- A later Linux parallel run intermittently failed immediately reopening a
  dropped Store with `Locked`. Unix flock ownership survives in a temporarily
  inherited or duplicated file description. A held-descriptor regression made
  this deterministic. Store now explicitly unlocks only in its creator process;
  inherited instances reject operations and cannot unlock a live parent's store
- Public phase-3 Windows CI failed the strict concurrent-reader atomic-replace
  test with OS error 5, Access denied. The previous tempfile persistence path
  used legacy MoveFileExW. On Windows, recovery now clears temporary attributes,
  retains RAII cleanup and flush ordering, and calls Rust 1.99's maintained
  std::fs::rename path with its modern POSIX-semantics fallback

The tests were not weakened: concurrent readers must still see a complete old or
new record. Added coverage holds a delete-sharing reader open during successful
replacement, and verifies that a Windows handle denying delete sharing causes a
visible failure with old data intact; a newer-sequence retry works once released.
No delete-first or sleep/retry workaround is used in production storage code.
These are availability fixes; the changes do not rewrite already acknowledged
records or bypass real ownership contention.

Fail-fast is disabled in the Linux/Windows CI matrix so one failing platform no
longer cancels the other platform's diagnostic run.

## Local verification of this exact source

- Formatting and strict workspace/all-target/all-feature Clippy: PASS
- Full Linux workspace suite: **249 ordinary tests passed**, zero failures;
  4 opt-in cases ignored
- Explicit separate-agent process test: **1 additional PASS**, one of the 4
  opt-in cases above
- Filesystem, LSP and asynchronous task Python process-chain smokes: all 3 PASS
- Complete Windows MSVC target/all-target/all-feature cargo check: PASS
- Full Linux release build: PASS; recorded in `release-hotfix-0.3.1-build.txt`

The recovery crate has 37 Linux tests, including actual pre-exec FD inheritance,
held-clone lifetime, inherited-instance rejection, strict reader replacement,
and 250 immediate reopen operations alongside 80 parallel fork/exec launches.
Before the final held-reader test was added, 30 repeated 36-test Linux suites
passed, totaling 7,500 immediate reopens and 2,400 fork/exec launches.

Raw local logs: `verification-hotfix-0.3.1-log.txt`,
`windows-hotfix-0.3.1-check.txt`, `release-hotfix-0.3.1-build.txt`.
The initial local script invocation used a custom Cargo target directory without
its expected target path; its explicit agent launch failed as a test setup error.
After providing that path, the full script above passed; no source assertion was
relaxed.

## Real Windows CI is a separate release gate

The source is published so GitHub's Windows runner can exercise the new runtime
cases. A cross-compile is not evidence that this Windows fix works at runtime.
This checkpoint must pass both Linux and Windows jobs for its own public commit
before it is described as CI-validated.

- Repository: [LLLLimbo/cedar-ide](https://github.com/LLLLimbo/cedar-ide)
- The failing earlier phase-3 Windows job:
  [run 37625344235, job 112805556824](https://github.com/LLLLimbo/cedar-ide/actions/runs/37625344235/job/112805556824)
- Earlier phase-1 and phase-2 Linux/Windows workflows passed after publication;
  they do not cover the phase-3 recovery implementation

Windows GUI behavior, Windows ACL and power-loss durability, macOS runtime,
authenticated SSH interoperability, and full IDE parity remain unverified.
The native Linux phase-3 UI and separate JVM/resource observations remain
historical evidence, not a new hotfix performance benchmark.
