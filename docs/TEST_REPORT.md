# Verification report · checkpoint 9D Java task ownership / 0.8.16 · 2026-10-08

This checkpoint adds real Java/task concurrency and forced-agent ownership
acceptance. Normal Windows language capability remains disabled pending the
finite activation checks and validation of the production route.

## Verified public baseline

Exact public 0.8.15 commit
[`74c130455d3fbc7ed6a6299c140ba5940b456426`](https://github.com/LLLLimbo/cedar-ide/commit/74c130455d3fbc7ed6a6299c140ba5940b456426)
[passed Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37721943175).
The direct real Java probe passed all three sessions in 39.710 seconds.

All three real agent/headless-editor sessions passed exact source diagnostics
and definition, completion/import resolution, atomic edits, actual Document
undo/redo, versions 2/3/4 and correction diagnostics. Version5 acknowledgements
were exact; correction receipts matched in 745, 810 and 504 ms with the unique
warning and no residual errors. Independently retained Java handles observed
natural exit0, with shutdown taking 525, 695 and 1,017 ms. The agent exited0,
source bytes remained unchanged, all observed roots exited and the generated
fixture was removed.

The earlier intermittent correction failure's cause remains unproven. This
successful run establishes the recorded baseline; instrumentation alone is not
claimed as a causal repair or a universal reliability guarantee. Existing native
mock/process suites separately cover descendants, Job-zero, cancellation and
adversarial stream ownership. These are not OS-window or authenticated SSH tests.

## Added finite ownership cases

The existing three-session editor test gains two independent-task checks. A
known task must remain live after the first Java session stops naturally. During
the second Java session, task cancellation must leave the same Java root live
and able to answer a real source hover query. Each task uses a retained native
identity and an exclusive lifetime-file lock; completion requires exit and lock
release before the safety cap.

A separate ignored test starts real Java and a known task, proves both live,
then kills only the owned agent. It requires that exact agent to be reaped and
both retained Java/task handles to signal, with the task lock released. Forced
termination is not labeled graceful. Failure paths retain explicit ownership and
cleanup checks before generated source removal.

The new native fixture mode has a fixed 210-second self-exit cap and an expired
marker. Existing five-second fixture modes remain unchanged. Neither cap exit124
nor marker presence can count as successful cleanup. The new case uses a single
known task root; already verified mock/native tests supply descendant and Job
accounting coverage rather than claiming a real JVM descendant tree was observed.

Typed concurrency and forced-cleanup receipts pass through the agent-only
sanitizer whitelist. Raw messages, paths, source and errors are not added to
public evidence. PowerShell runs the independent forced-owner case even if the
editor case fails and preserves both exit codes. It requires successful receipts
for both, in addition to all existing direct/editor checks. The containing CI
budget becomes twelve minutes for the added workload; individual Java semantic,
grace and cleanup deadlines are unchanged.

## Activation boundary

Exact native CI for this checkpoint is pending. Successful completion will close
the real task-concurrency and forced-owner cases, reusing existing trust/default
and mock cleanup evidence. Scoped production Java support must still be exercised
through the normal bundled agent and capability-enforcing Client before it is
advertised. Fixture-only markers and validation flags do not establish that path.

Native GUI interaction, authenticated SSH, performance benchmarking and broader
IDE feature coverage remain separately disclosed work. They are not blanket
prerequisites for the scoped Java route. No private diagnostic findings are
included here.
