# Verification report · checkpoint 9B / 0.8.2 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

This checkpoint implements the Windows owned language transport. **Windows IDE
LSP remains disabled.** It changes neither workspace trust nor the agent's
sequential scheduling contract. Git, synchronous Run and DAP remain disabled on
Windows. Native GUI Trust-on and authenticated SSH acceptance remain unperformed.

## Implemented

- One joined worker owns the server Job, stdin writes and both output streams.
- Incremental Content-Length parsing bounds retained headers/bodies before growth,
  handles arbitrary fragmentation and rejects truncated frames.
- Per-frame assembly deadlines do not slide with trickled bytes; outbound
  deadlines include queueing and every chunk. A possibly partial write poisons
  the connection. Caller notification timeout conservatively aborts even if its
  delivery state is unknown.
- Stop has an independent atomic flag/wake; no detached Windows reader/writer
  threads. Cleanup terminates the Job and joins outstanding kernel I/O.
- Root exit terminates descendants and drains pending/buffered output for at most
  250 ms before failing unresolved requests. A zero-progress overlapped poll is
  not mistaken for EOF. Final buffered responses are not discarded after one
  bounded capture round.
- Stderr is drained, optionally retaining only a 16-KiB terminal diagnostic tail.
- Twelve opt-in native tests cover independent concurrent sessions, root and
  descendant lifetime, stdout versus stderr EOF, stalled/trickling frames,
  blocked writes, proven queue saturation and waiter wakeup, bounded stderr,
  final response/exit, and repeated host handle/thread counts after joined cleanup.

See [the detailed transport contract](WINDOWS_LANGUAGE_TRANSPORT.md) for retained
memory accounting, platform differences, notification semantics and scheduling.

## Phase 9A native failure and targeted correction

Phase 9A public commit
[`304fa795743348b09514aec0c00767761430ee11`](https://github.com/LLLLimbo/cedar-ide/commit/304fa795743348b09514aec0c00767761430ee11)
had successful Ubuntu CI but failed one of 21 Windows lifecycle cases; the later
agent step was skipped. The [Windows job](https://github.com/LLLLimbo/cedar-ide/actions/runs/37688400137/job/113022041546)
reported `Written(65536)` where the early-exit test demanded a pipe error.

The test incorrectly treated an asynchronous Pending observation as proof that
subsequent transport acceptance was impossible. It did not establish process exit
before judging that completion. [Microsoft's WriteFile contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-writefile)
distinguishes pending asynchronous completion from completed transport writes;
completion does not establish application consumption. The exact instant of
kernel acceptance in that run is not established.

The corrected test accepts only one bounded Written completion or a documented
pipe-disconnection error. It verifies that a completed result is not replayed,
then observes exit code 23 through the owned process handle. Only after that
observation, it requires a distinct new probe to fail, persistent Closed state,
refusal of further writes, idempotent close/cancel and an empty Job. Production
primitive semantics were not weakened or changed. A fresh exact-commit native
Windows pass is still required; the failed run is not described as passing.

## Local checks

- `scripts/verify.sh`: 539 passing Rust test executions, zero failed; two public
  export regressions and five Python agent/tool chains passed. The six opt-in
  stdio process cases were explicitly executed. Nine ignored entries in the
  initial Linux aggregate remain separately scoped; opt-in Java/DAP tests were
  not executed by this aggregate.
- Strict whole-workspace, all-target/all-feature MSVC clippy passed. This is
  cross-target compiler evidence, not a Windows runtime pass.
- Linux release whole-workspace/all-feature build passed.
- Independent read-only reviews found and helped correct the root-exit buffered
  response race and invalid fixture scalar parameters. Queue saturation now
  requires observed QueueFull below the pending-request cap before abort.

Raw local evidence: `verification-phase9b-aggregate.txt`,
`verification-phase9b-windows.txt`, `verification-phase9b-release.txt`. These
machine-specific raw files are omitted from the source-only public export.

## Still required before Windows IDE activation

The CI workflow now explicitly runs the twelve ignored native transport tests,
plus all stdin/process and isolated-agent regressions on Windows. Actual success
must be tied to the published source commit. Compile checks, ignored tests and
missing/skipped dependencies do not count.

Subsequent work must verify real Windows Java diagnostics/completion/resolve/
hover/definition/correction/shutdown/restart, then isolated-agent concurrent task
and language ownership, independent stops and owner death. Trust remains a
separate explicit requirement. The synchronous agent can still wait up to 60 s
inside LanguageStart; transport ownership alone does not enable a peer stop,
file request or EOF to overtake initialization. No responsive-start claim is made.
