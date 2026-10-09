# Deliberate idle Disconnect

Use **Open workspace → Disconnect (keep drafts)** to release an idle connection
without opening another one. This action requires no execution trust and sends
no new workspace operation. It does not automatically Stop or Cancel tools.

First finish any save, Git, command or language request. Stop a running language
server and verify its cleanup; cancel an active command and wait for its terminal
outcome. An unknown command outcome still blocks Disconnect even if its warning
was acknowledged. Unverified or malformed language cleanup remains a blocker.
Read-only List, Read and Search work may be cancelled when Disconnect is admitted.

## What stays open

Draft text, saved baselines, document identities, full selections, Undo/Redo,
local recovery ownership, unsaved command configuration and interrupted-save
identities remain. Back/Forward continues to work offline for eligible unchanged
open buffers. This does not save workspace files or create a new recovery promise;
ordinary recovery limits still apply. You can continue editing while disconnected.
Saving requires a new explicit connection and the existing save protections.

Disconnect starts when its enabled action is accepted, after the frame's normal
ordered response processing. A save acknowledgment or read already completed by
that point keeps its normal effect. Later read results cannot open tabs, move the
selection, adopt disk baselines or launch follow-up work after that intent.
Pending navigation and disk-review actions are cancelled; drafts are retained.

## Waiting and cleanup results

**Disconnecting** means the existing background worker still owns local cleanup.
Connect, restore-and-connect and window-close actions wait for its terminal
receipt. A repeated click, cancelled Read or pipe EOF is not that receipt.
No new cleanup watcher or UI-thread wait is created.

For a process transport, the existing direct-child reaper still gives its child
the same two-second graceful opportunity. The worker observes its result for
three seconds. This is an observation budget, not a guaranteed end-to-end
Disconnect bound: process creation, embedded local filesystem work, OS scheduling
or kernel cleanup can remain non-interruptible.

A confirmed result establishes local connection-owner cleanup. For SSH this only
covers the local SSH client; it does **not** verify remote agent/task cleanup,
join detached pipe-reader threads or prove a graceful language-server shutdown.
A timeout, wait/kill error or worker unwind produces **Cleanup unverified**, never
a success receipt. An observation timeout leaves the existing reaper responsible
for its pending cleanup. Wait/ownership errors remain unverified; Cedar does not
retry cleanup by PID.

After a terminal result, **Reconnect** remains an explicit action. It creates a
fresh connection generation and retains the normal workspace/draft guards. If
local cleanup was unverified, a persistent warning remains even after reconnect;
a new successful connection does not certify the old process's exit. No request,
write, command or language startup is replayed automatically.

The warning describes observed cleanup failures from this explicit workflow and
unexpected worker exits while connected. It is not a complete historical audit
of every passive transport failure. Actual authenticated SSH and remote network
loss remain separate, unverified acceptance boundaries.

## Validation scope

The required acceptance uses generated cloud fixtures, normal agents and
controlled stdio peers on Ubuntu and Windows. It checks file/source preservation,
operation ledgers, selection/Undo/recovery retention, cancellation of a stalled
Read, a genuine short test-only cleanup-observation timeout, stale receipt
isolation and explicit reconnect. Native Windows GUI, SSH credentials/listeners
and user-machine operations are outside this checkpoint.
