# Frontend recovery and asynchronous command interactions

## Private local recovery

Recovery is **on by default for each editor session**. The footer always shows
its state and opens the Recovery window; the header also has a Recovery button.
The window explains where copies go and offers a per-session toggle. Turning it
off cancels queued writes, retains existing copies, and does not interrupt a
write already in progress. Restarting Cedar starts a new session with recovery
on. No remote source is transmitted anywhere except the explicitly selected
workspace operations; recovery itself writes only to the frontend's local store.

Use `CEDAR_RECOVERY_DIR` for an explicit absolute recovery folder. Tests always
use temporary synthetic folders. Production defaults and platform privacy and
power-loss boundaries are described in `../../docs/RECOVERY.md`. In particular,
Windows inherits folder ACLs; the UI does not claim Unix-equivalent owner-only
permissions. Drafts are plaintext and can themselves contain secrets. No
connection credentials or execution-trust settings are serialized.

A dedicated background actor coalesces each workspace/path's latest snapshot
with a one-second trailing debounce. It holds at most 64 pending keys and
128 MiB of pending text. One in-flight I/O operation is additional. Mutation IDs
are monotonically increasing across all documents in this frontend. Disk and
UI acknowledgements must match the latest operation and exact document/edit/base
revision before the footer says **Draft backed up locally**. New typing, saves
that update the base revision, removals, and actor replacement invalidate older
acknowledgements. Failed, full, corrupt or locked storage never disables editing;
Recovery exposes the issue and Refresh/Retry controls. No old copy is evicted to
make space.

Available copies are shown on startup with project, path, UTC timestamp and
size. Review restore reads one bounded copy; **Connect with trust off and restore**
is a second explicit choice. The remote endpoint, root and agent path are shown.
Connection is never automatic. The agent's canonical root must match the intended
identity. An existing tab is never replaced by recovery. Restoring retains the
original saved text and optional base revision, does not reload from disk, does
not save to the workspace, and does not launch language services or commands.
Even a cached review requires a fresh matching storage write acknowledgement
before it can be described as backed up.

An unreviewed older recovery is never overwritten by ordinary edits to a freshly
opened disk tab, and is not removed by saving/discarding that unrelated tab.
The status says **Older recovery waiting** until the user explicitly restores or
removes it. Saving or discarding a tab affects only recovery records owned by that
tab. Saving while typing updates the protected base and preserves newer draft text.
An explicit Remove copy action can be followed by a new copy if that file remains
open, dirty, and recovery is on; the confirmation explains this.

Dirty-close protection remains authoritative. Discard-and-quit waits for owned
recovery removal acknowledgements. New input during that wait cancels the close
and requires another discard confirmation. The Close viewport command is emitted
only after the entire input frame has finished. Dropping the actor on an orderly
exit flushes queued work; an unexpected process exit can lose text since the
latest acknowledged snapshot, including the debounce window.

## Commands

Commands uses `RunStart`, `RunPoll`, and `RunCancel`. A command is run only after
an explicit click, in a connection whose execution trust is enabled. The phase-5 form uses an
executable, ordered literal argument rows and a 1–300-second timeout. Explicit
saved profiles reuse this dispatch and the ordinary document save path; see
[Task profiles](../../docs/TASK_PROFILES.md). There is no implicit shell,
automatic retry/restart, or recovered execution trust.

The UI accepts one task per connection, polls at most once per 250 ms while it is
nonterminal, and replaces full bounded output snapshots instead of appending
repeated text. Editing, opening and saving files can continue. Starting, Running,
Cancelling, Succeeded, Failed, Cancelled, Timed out, Output limit, and Spawn failed
are distinct visible states. Cancel requests cancellation and waits for a terminal
status; natural completion can win the race.

Close and reconnect attempts during a running command open an explicit
Cancel-and-wait choice. When the command is terminal, the user retries the intended
close/reconnect, which reruns dirty-file and pending-mutation protections. If the
connection or cancellation result is unknown, the UI says the command may still
be running and requires explicit acknowledgment before close/reconnect. It does
not automatically start anything again. Task IDs are cleared on connection change;
late connection generations and task epochs cannot update a new task.

See `../../docs/RUN_TASKS.md` for process ownership, output caps and platform limits.
