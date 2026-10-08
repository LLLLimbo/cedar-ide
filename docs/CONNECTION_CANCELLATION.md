# Connection and read cancellation

Cancel Connecting and replacing a connection now signal the pending process
transport, including its initial Hello. A permanent token belongs to one
connection generation; replacement connections receive a fresh token.

Only Hello, List, Read and Search observe cancellation while waiting for a
response. They wait in slices of at most 50 ms against the original absolute
request deadline. A slice timeout is not an error and cannot extend that deadline.
Cancellation is checked before accepting a response and before enqueueing another
operation. Old-generation events cannot update the replacement workspace.

A Write, Git, language or task operation already enqueued to the process transport keeps its ordinary
response and deadline. Cancellation does not make its result safe to replay.
Commands still waiting in the old app worker queue are discarded.
The existing frontend guards still prevent reconnect during saves, language
operations and active task transitions. Drafts, Undo and interrupted-save tokens
retain their existing protection.

No new thread or channel is introduced. Only an eligible pending request polls;
an idle connection does not. Existing callers of the default Client constructors
retain their normal blocking waits. The opt-in API returns transport_cancelled
and closes the connection through its existing owned-child reaper.

## Timing and ownership limits

The polling interval is not a wall-clock guarantee: scheduling, process creation,
filesystem calls and kernel cleanup may take longer. Embedded Unix workspace
operations remain synchronous and cannot be interrupted inside Workspace::handle.

Cancellation releases the caller before asynchronous child cleanup necessarily
finishes. Client::close_and_wait separately verifies the direct owned child's
reaper result. The transport still has detached pipe threads; this API does not
claim those threads are joined or that arbitrary descendants holding inherited
pipes have exited. Tests distinguish caller cancellation, direct-child reaping
and bounded resource-count settling.

## Validation

Marker-gated synthetic peers publish readiness only after receiving a complete
wire request. Tests cover initial Hello cancellation, stalled reads, discarded
queued work, permanent and fresh tokens, response races, and authoritative replies
to operations that cannot be interrupted. Production request deadlines remain
in force, so a short test watchdog cannot pass through the ordinary timeout.

Separate app tests exercise actual cancellation and generation handling without
enabling GUI execution trust. An isolated test process measures repeated cycles
after warmup and exact reaper completion. These tests do not authenticate to SSH
or replace native GUI acceptance.
