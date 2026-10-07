# Verification report · checkpoint 9A / 0.8.1 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar 0.8.1 adds the owned Windows stdin primitive required for future language
transport. It does **not enable Windows LSP**, change workspace trust, add protocol
operations or alter task stdin from NUL. The optional piped constructor is used
only by its isolated tests in this checkpoint. Git, legacy synchronous Run and
DAP remain outside Windows activation.

## Changes and ownership contract

The existing suspended, atomically Job-assigned process constructor retains NUL
stdin. An explicit piped variant adds a private overlapped parent writer and a
synchronous inherited child reader. Exact stdio HANDLE_LIST, current-logon DACL,
first-instance/local-only naming, connected-peer verification and Job-before-resume
remain in force. No global permission or network setting is changed.

One pinned 64-KiB user buffer, OVERLAPPED and event remain owned until completion.
The caller can mutate/drop its source after submission. Begin/poll report the
actual completed count exactly once; another submission while pending is refused.
Short completion leaves a caller-owned suffix. Empty application input is not
an OS null write or EOF. Stdin close refuses an outstanding write, then closes
without FlushFileBuffers or DisconnectNamedPipe, preserving accepted unread data.

Cancel is followed by observed completion before freeing storage. The result
distinguishes no pending write, completion winning with a byte count, and completed
cancellation with possibly delivered bytes. A framed protocol must abandon a
partially transmitted or failed connection, not replay the whole message. The
primitive does not implement framing, fairness, deadlines or a write queue.

Drop requests Job termination, completes stdin I/O, completes both captures even
if stdin cleanup reports an error, then waits for the root and releases handles.
An exceptional kernel delay can postpone completion; a timer never authorizes
freeing memory still referenced by I/O. Pipe kernel-buffer reservation is advisory,
so the 64-KiB user allocation is not an exact kernel-memory claim.

A cached creation-time process ID is exposed for diagnostics required by the
existing language API. It is not liveness evidence or authority for lookup or
termination. All process cleanup still uses owned handles. An existing lifecycle
case compares the ID to the fixture and held observation handle, then checks it
remains the original identity after exit.

See [process design](WINDOWS_PROCESSES.md) and the [crate contract](../crates/winprocess/README.md)
for Win32 references and host-wide inheritance restrictions. The supported host
remains the controlled-spawning isolated agent; this is not a hostile-code sandbox.

## Review and test design

Independent read-only review found no blocking ownership, inheritance, completion
or failure-path issue. Its remaining coverage suggestion was incorporated: the
pending-write round trip now verifies actual Pending status, mutates and drops
the caller's heap buffer while still Pending, drains the receiver and checks the
original bytes exactly. It does not control Windows' internal copy timing.

Nine added Windows library tests cover bounds/empty chunks, copied buffer lifetime,
closed-reader rejection, full-pipe cancellation, completion-winning cancellation,
thread transfer with joined cleanup, inherited flags, name collision and early
client/partial-connect failure. Short-success handling has a deterministic count
validation test; no actual OS short write is claimed.

Eight added opt-in lifecycle tests cover multichunk binary fidelity, distinct
stdout/stderr and EOF, unchanged NUL behavior, cancellation while the nonreading
child remains alive, early root exit, pending-I/O tree Drop, independent owners
and unrelated handles, owner crash, and repeated handle-count checks. Known
processes have held observation handles; whole-Job cleanup still requires zero
members. Tests do not mistake a fixture's safety-cap exit for successful cleanup.

## Final local verification

- **521 Rust passes**: 515 ordinary plus six explicit process tests, zero failures
- Five actual-agent Python chains and two public-export regression tests passed
- Rustfmt and strict Linux/MSVC whole-workspace, all-target/all-feature Clippy passed
- Linux release build and all six tests against the final release agent passed
- All 343 upstream Cargo.lock records unchanged; ten workspace versions bumped

Linux runs thirteen portable winprocess library tests and one fixture-format test;
**it executes none of the new Windows-specific behavior**. MSVC cross-target
Clippy compiles the Windows units and lifecycle target, which is not runtime
acceptance. The aggregate's nine ignored entries include six explicitly rerun
process tests and three historical runtime/font opt-ins not claimed as passes.

Final logs: [aggregate](../PUBLICATION.md#verification-evidence),
[MSVC](../PUBLICATION.md#verification-evidence), [release](../PUBLICATION.md#verification-evidence),
[release-agent integration](../PUBLICATION.md#verification-evidence).

Linux release SHA-256 (unsigned, not Windows executables or reproducibility proof):

- cedar: `47d86b7660e7a031ebc97b72a038500f4ff72a84f0dd07ea0c08b568e8ab5bde`
- cedar-agent: `5619d41d468dd31e1ed4dbba60dac2d4f1af529d0d5823efdbf4b3c9f9c0899d`

## Exact-commit Windows gate

Publish the reviewed source checkpoint and run its Ubuntu/Windows CI. Windows
must actually execute all **48 winprocess library tests**, **21 primitive
lifecycle tests**, and the existing **seven agent/bundle tests**, followed by
release/artifact success. None may be inferred from cross-compilation or an
ignored test declaration. The existing CI lifecycle command already selects the
new cases; this checkpoint does not broaden CI credentials or permissions.

The previous stage
[`5927b6c51d6bc4d4b88f676f4d5c27a3a314ce0f`](https://github.com/LLLLimbo/cedar-ide/commit/5927b6c51d6bc4d4b88f676f4d5c27a3a314ce0f)
passed [both platforms](https://github.com/LLLLimbo/cedar-ide/actions/runs/37685061425),
including disk-review/native-history regressions and thirteen plus seven Windows
runtime cases. Its Linux native GUI evidence belongs to [stage 8](TEST_REPORT_PHASE8.md),
not these new binaries.

Windows language transport will then use this primitive behind the existing
workspace gate and undergo its own native tests before agent/JDT activation.
The agent remains sequential: a joined transport alone does not make initialization
or queries interruptible by another agent request. The [language plan](WINDOWS_LANGUAGE_PLAN.md)
records this limitation and the real Java acceptance requirements.

Authenticated SSH and native Trust-on testing still await their existing approval.
Windows/macOS GUI, a new performance comparison and IDEA parity are not claimed.
The historical Linux lifecycle timeout has not recurred; its original cause
remains unconfirmed, with the original timeout bounds unchanged.
