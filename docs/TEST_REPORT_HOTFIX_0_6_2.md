# Verification report · 7A revision / 0.6.2 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar **0.6.2**, protocol **4**, revises two test fixtures after the first public
7A run. The process ownership implementation and all execution gates are
unchanged. **Windows tasks, Git, legacy Run and language services remain
disabled.** This revision still requires exact-commit Ubuntu/Windows CI acceptance.

## Evidence from the first 7A CI run

Public commit [`0ae9f54a1d35cb428a1b53f387773ce718174b24`](https://github.com/LLLLimbo/cedar-ide/commit/0ae9f54a1d35cb428a1b53f387773ce718174b24)
failed [the first 7A CI run](https://github.com/LLLLimbo/cedar-ide/actions/runs/37656647369):

- Windows: 38 of 39 `cedar-winprocess` library tests passed. The occupied-pipe
  name test expected error 5 but received error 231. The dedicated 13-test
  lifecycle step was **not reached**, so it has no runtime acceptance yet
- Ubuntu: workspace tests and release build passed, then the explicit lifecycle
  test `real_agent_eof_broken_pipe_and_malformed_input_unwind_active_tasks`
  exceeded a four-second condition deadline. Five other process integration
  tests passed. Its old assertion did not identify the case or awaited condition

The [original 7A report](TEST_REPORT_PHASE7A.md) records the evidence available
when that candidate was sealed. It is not proof that the later CI passed.

## Windows collision assertion

The server permits only one pipe instance and uses FIRST_PIPE_INSTANCE.
[CreateNamedPipeW](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-createnamedpipew)
documents first-instance rejection; Windows also defines
[ERROR_PIPE_BUSY (231)](https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--0-499-)
for exhausted pipe instances. Which condition Windows checks first is an
inference from the observed result, not a promised ordering.

The test now accepts **only** ERROR_ACCESS_DENIED (5) or ERROR_PIPE_BUSY (231).
It separately requires the server-creation call to fail before any connection
attempt, then requires the full capture constructor to fail. It verifies the
original client PID, exact bytes written before and after both collision
attempts, and EOF within five seconds once the original writer closes. An
arbitrary construction error, stolen stream or leaked writer cannot pass.
Production pipe naming, DACL, ownership and connection checks are unchanged.

## Linux fixture and diagnostics

The original timeout was not reproduced locally: 200 executions of the focused
failure test plus 100 executions of the six-test integration suite passed.
That is **800 test executions and 1,200 four-case fault scenarios**. These are
stress repeats, not additional unique coverage, and do not establish a root cause.

Inspection found a real fixture race: accepting two whitespace-separated tokens
could accept a partly written second PID, followed by a second unchecked read.
The fixture now publishes its PID record by rename, requires the final newline
and two valid nonzero PIDs, and retains that single complete snapshot. This
removes the visible race; it does **not** prove that race caused the CI timeout.
Fault injection additionally requires both recorded processes to be alive.

Timeouts now identify each case and condition and include the observed process
state, wait channel, leader presence, exit status and captured agent stderr.
A reader continuously drains stderr while retaining at most its first 16 KiB,
so diagnostic output cannot fill the pipe. Exit waits for stderr-reader completion
before joining it. The original **four-second** condition deadline, all four
fault cases, both-processes-inactive requirement and leader-reaped requirement
remain intact. No production Unix cleanup or transport code was changed.

An independent source review found no blocking issue in either revised test.
If the original timeout recurs in CI, use the new evidence to distinguish fixture
readiness, agent shutdown and task cleanup; passing a rerun alone is not root-cause
proof. Arbitrarily increasing the timeout is not the proposed resolution.

## Local verification of this revision

- Final aggregate: **473 Rust passes** (467 ordinary plus six explicit process
  tests), zero failures; nine aggregate opt-in cases, six subsequently run
- Five real-agent Python chains and two public-export regressions passed
- Rustfmt and strict whole-workspace Linux Clippy passed
- Windows MSVC whole-workspace all-target/all-feature strict Clippy passed
  (cross-compilation, not execution)
- Linux release build and release-agent six-test integration passed
- No new native GUI, real JDT/Kotlin, memory benchmark or authenticated SSH run

Logs: [aggregate](../PUBLICATION.md#verification-evidence),
[Windows cross-target check](../PUBLICATION.md#verification-evidence),
[release build](../PUBLICATION.md#verification-evidence),
[release-agent integration](../PUBLICATION.md#verification-evidence).

Only the ten workspace package versions changed in Cargo.lock; all 343 upstream
packages remain fixed. The review does not establish Windows GUI acceptance,
IDEA feature parity, a performance advantage or authenticated SSH interoperability.

Linux release SHA-256 (unsigned, not reproducibility or Windows attestations):

- `cedar`: `c6e1a5c1d13ff43a5fd6f1efd664b40a70f126aa4f043d6f2f9a902f10f40239`
- `cedar-agent`: `25cc88dfa0c5aa8a621cbc672a64e35e4a0f85f8f5e7b203accf4de1a39f8788`

## Required next gates

1. Publish this audited revision and run Ubuntu and Windows CI on that exact SHA
2. Obtain all 13 Windows lifecycle results, including owner crash, live-tree
   Drop, nested cancellation, inherited-handle exclusion and repeated handle counts
3. Only after primitive acceptance, integrate Windows asynchronous tasks in a
   controlled-spawning isolated agent; preserve explicit trust and keep Git,
   legacy synchronous Run and persistent language/debug services disabled
4. Authenticated SSH and native trust-on tests still need their existing approval;
   test-only processes do not substitute for those separate gates
