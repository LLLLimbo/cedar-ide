# Verification report · scoped Windows Java production route / 0.9.0 · 2026-10-08

This checkpoint adds an explicit Java/JDT route to the normal Windows isolated
agent and Language panel. Its exact native production-route verdict is pending.

## Verified public prerequisites

Exact public 0.8.16 commit
[`af15a82c414f4081077429f04736e58e001545cf`](https://github.com/LLLLimbo/cedar-ide/commit/af15a82c414f4081077429f04736e58e001545cf)
[passed Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37724199678).
The direct real Java probe passed three sessions in 40.020 seconds. All three
real agent/headless-editor sessions passed exact semantics, completion/import
resolution, atomic apply, actual undo/redo, version synchronization, correction
and independently observed natural exit0.

Two independent task lifetimes passed: Java Stop preserved a live task, and task
cancellation preserved the same Java session and real hover. A separate forced
owner case completed in 6,397 ms: after killing only the exact owned agent, the
retained Java/task handles signaled, the task lock released, no safety cap fired,
source stayed unchanged and the fixture was removed. Child exit0 after injected
owner death was recorded as forced cleanup, not graceful shutdown. Existing
native/mock suites cover descendants, Job-zero, EOF and cancellation ownership.

The earlier intermittent correction failure's cause remains unproven. Later
successful runs establish their recorded outcomes, not a causal fix or universal
reliability guarantee. Native GUI and authenticated SSH remain separate claims.

## Production behavior

Protocol4 gains a capability-negotiated `LanguageStartJava` operation with three
explicit host paths: Java executable, JDT distribution and external data directory.
Only native Windows isolated agents advertise `language_start_java`; generic
Windows language startup stays unsupported. Execution trust is checked before
filesystem inspection or spawning. Old peers never receive the unsupported new
operation. Shared language operations work with the selected startup capability.

A shared recipe validates ordinary native paths and identity, chooses the Unicode
distribution cwd, and supplies the exact relative launcher plus encoded location
URLs. It does not download tools, create data directories, use PATH or a shell,
or accept arbitrary JVM argument text. Production initialization does not claim
class-file viewing, and production Stop does not run a semantic/indexing query.
Fixture markers, crash hooks and the fixed String witness remain fixture-only.

The Language panel exposes the three host paths, fixed Java document mode and
current import/viewer limitations. It preserves explicit trust and synchronizes
Java documents only. Queued startup/query behavior is disclosed. Java sessions
receive a 75-second client request budget; generic language/task budgets retain
their previous values. See [configuration and limitations](WINDOWS_JAVA_SETUP.md).

## Honest termination and cleanup

A durable Windows shutdown report separates protocol completion, first terminal
cause, root exit observed before/after owner termination, material transport
failure and joined cleanup errors. It never infers natural exit from a numeric
exit code alone. A final malformed frame after root exit vetoes graceful status.
Original failures, worker panics and outcomes remain cached across repeated calls;
a consumed join handle cannot manufacture later success or extend grace.

The UI renders bounded natural/forced/error summaries. Unverified or malformed
Stop evidence blocks restart and cancels pending window close rather than hiding
the error. Explicit Client close waits for a verified owned-child reap result;
wait/kill errors remain failures. This does not claim detached transport-reader
joins or an independent real-JVM Job inventory.

## Required exact native validation

The existing direct and strict three-session fixture acceptance remains required.
An added ignored test uses the normal shipping agent and capability-enforcing
Client, without validation markers. It checks Java-only capabilities, generic and
untrusted start rejection, retained Java image/identity, real source/editor
semantics, unchanged disk bytes, typed Stop outcome against the native handle,
verified client-owned reap and generated-root removal. An honestly reported
joined forced Stop may satisfy this production cleanup check; it is not counted
as graceful. The stricter natural-exit fixture is preserved separately.

Only typed fixed enums, bounded numbers and booleans pass through the sanitizer.
Raw source, URI, messages, stderr and JVM diagnostics remain private. The script
runs independent fixture, forced-owner and production cases and preserves all
three exit codes; each has a required receipt. Native bundle discovery remains
covered by its existing separate suite. No private diagnostic findings are
included in this report.
