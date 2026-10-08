# Verification report · checkpoint 9C ordered shutdown / 0.8.10 · 2026-10-08

This checkpoint repairs the exit-message/stdin-EOF ordering and adds explicit
bounded Windows graceful-exit handling. The normal Windows IDE language gate
remains closed pending full agent/editor acceptance.

## Exact 0.8.9 result

Public commit
[`748edb0f12ef95fb535db5e620c0a47dd9ddafff`](https://github.com/LLLLimbo/cedar-ide/commit/748edb0f12ef95fb535db5e620c0a47dd9ddafff)
[passed Ubuntu CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37707935686),
including the corrected seven-case stdio suite. Windows real JDT completed its
initial semantic session in about 12.2 seconds: initialization, source open,
precise error diagnostics, hover, completion, definition, lazy import resolve,
unsaved correction, error removal plus correction-specific warning, source
preservation and document close. The intended data-directory witness passed.

Cleanup then observed the exact retained root handle signaled with exit code
1067 (ERROR_PROCESS_ABORTED), not zero. Timing was consistent with exhausting the
existing ten-second grace. Restarts and later agent-language cases did not run.
There was no real-JDT fatal log; only the three deliberately contrasted verbatim
executable diagnostic cases still crashed. Initial semantics are useful evidence,
not full acceptance. Forced termination is never relabeled graceful.

## Concrete lifecycle gaps and scoped repair

The client previously wrote exit but retained the server's stdin writer during
grace. A deterministic fixture now requires a complete exit frame followed by
stdin EOF before it can leave a witness and exit naturally. Closing only later
in Drop cannot meet that contract. The LSP shutdown path now uses a private final
write command: emit every exit-frame byte, close stdin, then acknowledge. Ordinary
notifications and generic process launch semantics do not change. Both portable
and Windows writers observe this ordering.

Windows also previously treated every stdout EOF as immediate teardown. During
an explicitly armed graceful phase, a server can legitimately close stdout before
its remaining shutdown work exits. Only clean EOF in that phase is deferred until
root exit or the original fixed deadline. Unexpected EOF outside it, malformed
frames, abort and I/O errors remain fail-closed. The phase is armed before exit
is enqueued to avoid a fast-child race; repeated arming cannot extend it. After
root exit, only an already-submitted final write may have its completion polled;
no new write or replay is permitted.

The early-stdout-EOF gap is independently tested; it is not presented as the
observed cause of 0.8.9's ten-second exhaustion. Whether ordered stdin EOF resolves
actual JDT shutdown is a question for the next exact native run. No grace interval
is increased. The existing shutdown API may still report joined cleanup rather
than natural exit, so the real-Java probe continues requiring independently
observed root exit zero and now records elapsed shutdown milliseconds and a
reconstructed terminal-category enum before Drop. Raw errors remain private.

## Regression and verification boundary

Four new native Windows cases cover complete exit plus stdin EOF/natural zero,
stdout EOF followed by synchronized natural root release, stalled graceful EOF
with bounded forced cleanup, and malformed EOF retaining protocol failure. They
retain actual root observation handles and prove descendant lifetime-lock release.
A portable fixture writes its audit witness only after complete exit and EOF.

Local aggregate verification passed 559 Rust tests, all five Python protocol/
profile smoke chains, two export tests and 39 sanitizer tests (two native Windows
skips). MSVC all-target/all-feature clippy and optimized workspace build passed.
Independent review cleared ordered EOF, fixed grace, completion-only post-exit
polling, fixtures and sanitized telemetry. Native execution remains necessary.

The local real-Java semantic regression completed, but orderly JDT shutdown
remains unresolved. This checkpoint makes no upstream root-cause or graceful
Java-exit claim. Further diagnosis and native verification are still required.

The next exact native run must pass these cases, real JDT's three complete
semantic/zero-exit sessions and later agent fixtures. GUI Trust, authenticated SSH
and actual IDE/headless Java edit acceptance remain separate pending boundaries.
