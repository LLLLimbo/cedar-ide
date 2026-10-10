# Verification report · Linux owned language transport / 0.36.0

The [0.35 agent package checkpoint](TEST_REPORT_PHASE35_LINUX_AGENT.md) is fully
verified. This slice repairs ownership of Linux generic LSP processes. Typed
Java/Maven startup remains Windows-only, capabilities and wire schemas unchanged.

## Required contract

One Linux owner manages a private process group and nonblocking parent stdin/stdout.
Inherited/discarded stderr behavior remains unchanged. Existing request, queue,
frame and graceful-exit budgets remain unchanged. Full exit-frame acceptance and
stdin closure precede its acknowledgement; uncertain partial writes are never
replayed. Buffered responses precede terminal closure, and malformed/incomplete
final capture cannot count as graceful.

The root remains unreaped while group/root signals are sent. Lost exclusive wait
ownership permanently disables subsequent cached-PID operations. Internal Linux
observations distinguish exit code from signal, pre/post termination observation,
root reaping, released parent I/O, worker join and bounded failure categories.
They are not Windows Job or independent process-group-empty evidence.

The first cleanup trigger fixes a three-second observation budget. Normal cleanup
joins an actually finished owner. Timeout remains cached as Unverified/not joined;
the same owner retains eventual wait responsibility, without a replacement watcher
or caller PID retry. A late first observer can verify cleanup recorded complete
within budget, but never upgrades a previously cached timeout. Recorded joined
elapsed time describes owner completion after cleanup began, not caller waiting.
This is not a hard real-time limit on uninterruptible processes or thread scheduling.

Unverified Linux generic Stop/initialization cleanup blocks replacement startup
within the workspace. Trust revocation still permits cleanup of an already-owned
Linux session only. Malformed Stop acknowledgements block frontend restart while
retaining drafts; only an explicit valid generic stopped acknowledgement clears
that session. Typed Java retains its richer outcome validation. Reconnecting does
not prove the previous process exited.

## Verification status

Final local checks passed on the cloud Debian 13 x86_64 host: 1,349 Rust tests
passed across 44 suites (47 opt-in/helper tests ignored by aggregate discovery),
strict host and Windows MSVC cross-target Clippy, formatting, all-feature release
builds and default-feature shipping binaries. This is not native Windows evidence.

The dedicated normal release-agent run executed all four Linux tests, including
blocked-write deadline cleanup with subsequent file access, independent owners and
Stop/restart, trust-off rejection, and cleanup after trust revocation. Three bounded
receipts record actual source preservation, root observations and agent reaping;
the legacy wire acknowledgement is not presented as an independent transport-worker
join witness. Internal transport and fault-seam tests separately cover joining,
identity loss, setup/unwind failures, inherited-stderr backpressure, and readiness
waiting after cleanup has already begun. The final aggregate includes the three
readiness regressions added after the earlier local run.

Package regressions passed: 30 Linux archive tests, nine Linux receipt tests, and
35 Windows-package tests with one platform-specific skip. Independent lifecycle
review cleared the final implementation. Exact-source Ubuntu 24.04 and Windows CI,
actual native process receipts and regenerated package verification remain pending.
No new distribution acceptance is claimed until those checks complete.

## Limits

No new listeners, SSH authentication/deployment, security settings, downloads,
Java/Maven activation or capability-limit change. Existing locked libc gains only
a Linux language-crate dependency edge. Process groups do not contain escaped or
credential-changed descendants; abrupt agent death and competing SIGCHLD reapers, SIGCHLD=SIG_IGN and SA_NOCLDWAIT
are outside the contract. Other portable platforms keep their existing transport.
