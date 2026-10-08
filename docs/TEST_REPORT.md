# Verification report · Cancellable Java startup / 0.17.0 · 2026-10-08

This checkpoint moves only explicitly requested typed-Java launch/initialization
into a workspace-owned startup lifecycle. Ordinary agent requests remain serial;
there is no general concurrent-agent rewrite. The previous [0.16.0 exact CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37795611035)
passed both platforms, including the separate unversioned Java refresh witness,
all earlier ownership/Git checks and the verified development ZIP. Its evidence
is retained in [the phase-16 report](TEST_REPORT_PHASE16.md). The intermittent
JDT diagnostic-publication cause remains unresolved.

## Finite contract

Protocol 4 adds optional Begin/Poll/Cancel capabilities with a positive startup
ID. Legacy synchronous `LanguageStartJava` retains its original contract.
Missing/partial capabilities cannot send an asynchronous launch; old compatible
agents remain usable through the clearly labeled synchronous path.

A single transient owner performs Java launch and initialization. Begin returns
Starting; ordinary reads, revision-checked saves and task controls can proceed
while initialization waits. Language queries remain unavailable until Ready. Ready denotes the initialization
handshake, not proof background indexing has settled; later queries remain synchronous.
The fixed eligibility deadline is 75 seconds from accepted Begin. Initialize's
response remains capped at 60 seconds; the initialized notification is clipped
to the same remaining absolute budget. Polling never extends that deadline.
Expiry requests cancellation; kernel/process cleanup may take longer and stays
visibly Cancelling until actually joined. Elapsed time is not cleanup evidence.

## Ownership and cancellation

The prepared client remains in a guarded slot owned by a live startup worker
until the same Poll atomically installs the tagged session. A completed worker
result never carries an unconsumed live client. No-poll timeout, cancellation
and panic unwind clean an unadopted slot. After adoption, cancellation addresses
only that startup ID; old IDs cannot stop a later unrelated session.

The abort signal bypasses the LSP lifecycle gate and outbound queue and does
not join. Cleanup runs off the agent request handler. Cleanup-thread allocation
failure retains ownership in a blocked fallback rather than synchronously
joining inside Cancel. Failed or unverified cleanup prevents another start on
the same connection. Exact existing-ID cleanup/status remain reachable after
trust revocation; Begin still requires trust before allocation/launch.

Ready/cancel crossings, repeated cancellation, stale IDs, worker panic, normal
Stop and Workspace Drop keep a single explicit owner. Windows terminal cleanup
requires joined transport/process/I/O evidence with no cleanup errors. Portable
fixtures do not manufacture Windows process-tree verification.

## Frontend and transport

Starting/Cancelling persists between short wire requests. The UI binds results
to connection generation, language session and startup ID, keeps Cancel reachable,
and sends at most one lifecycle poll at a time. It does not automatically replay
starts, edits or saves. Close/reconnect and pending-save safeguards remain active;
cleanup cannot close a window against a newer draft snapshot. Unverified cleanup
is visible and blocks restart. Old synchronous agents explicitly lack startup
cancellation; Stop becomes available after startup returns.

Client Java-session timing becomes active only after a valid matching Ready
response. Old snapshots cannot activate or clear a newer owner. Quick lifecycle
requests retain ordinary transport deadlines; authoritative save replies and
unknown-save semantics are unchanged. Malformed lifecycle replies fail closed
without automatic cancellation/restart traffic.

## Acceptance boundaries

Deterministic fixtures cover prelaunch and real stalled initialize, ordinary
reads/saves/conflicts and independent tasks, cancellation, original deadlines,
no-poll prepared cleanup, atomic handoff, exact old-ID isolation, panic positions,
allocation failure retention and Drop cleanup. Windows observers retain handles
while the fixture root/descendant are live and verify identity and signalled exit
on those same handles. The extra descendant starts before the initialize marker.
Existing native Job/pipe, EOF and forced-owner suites remain required.

The real-JDT Quick production test uses Begin, reads source while startup remains
pending, observes the exact owned root, and reaches Ready before executing all
original semantic/editor and explicit-refresh assertions. Its failure path
cancels an unadopted startup and still reaps the agent. The long resource profiles
retain synchronous startup and their existing workload. No new JVM or resource
benchmark is introduced; no resource-reduction claim follows from this change.

## Verification status

Final local aggregate: 777 Rust tests passed, with 22 opt-in/native cases ignored
in that aggregate and retained as explicit CI stages. The final suite includes
the allocation-failure regression and all client/UI consistency corrections.
Strict full-workspace host and MSVC checks passed with warnings denied. The
normal default-feature release app/agent build passed, followed by actual-agent
protocol, capability, language-bridge and task-bridge smoke checks.

Python checks passed: collector 70 with two platform skips, process-tree observer
79, GC collector 36, bundle 28 with one native-only skip, export two and Git
fixture six. Formatting and diff checks passed. Independent static review found
no remaining blocker in ownership, replay behavior, evidence or documentation.
These checks do not replace native execution.

Exact native Windows runtime tests, actual JDT, prior Git/ownership/file suites
and the regenerated default-feature bundle remain required. Cross-compilation is not
native execution. GUI and authenticated SSH acceptance remain separate gaps.
