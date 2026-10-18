# Window close and local connection cleanup

This contract was introduced in the 0.78 candidate and is unchanged by the 0.79.0
version/documentation checkpoint. The exact development parent
[`817c341`](https://github.com/LLLLimbo/cedar-ide/commit/817c341eacc54645ce9376f3b6dfa0359782efb5)
has [passing four-job CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/39124400977)
and bounded cloud-Linux native runs; it is no longer an uncompiled source stage.
Those observations do not establish a comprehensive native close/cleanup matrix
or a fix for the historical unattributed zombie. Final 0.79.0 results remain
pending at source sealing and require their own external receipt. The following
describes the contract, not a platform-wide validation guarantee.

Native window Close keeps the existing task, language, mutation and exact draft
recovery guards. Once those approvals are confirmed, the app retires its current
connection handle and waits for independent local-owner completion receipts. It
tracks every registered current and retired owner for this app lifetime, across
cancellation, replacement, restoration and reconnect. Ordinary late worker UI
responses cannot navigate, mutate drafts or launch work while exit is waiting.

Only a final non-discarded update pass can emit Close. That pass checks the same
recovery ticket and exact draft guard again, after the other frame finishers, and
requires every owner to be settled without uncertainty. Repeated Close requests
do not start duplicate cleanup. EOF, an absent Worker, elapsed time, a new
connection's success and text supplied by a peer are not cleanup receipts.

## Keep editing and blocked exit

The waiting dialog keeps **Keep editing** above its scrollable status. Choosing
it, or closing that dialog, cancels the exit intent and retains drafts and Undo.
If transport retirement already began, editing resumes disconnected: reconnection
is explicit and does not resurrect the old worker or its queued requests. Late
owner receipts still update the lifetime ledger but cannot close the window on
their own. Connect and restore-and-connect remain gated while close is pending.

A pending owner continues to block normal exit. The five-second waiting notice is
an observation cue, not a deadline that changes the result to success. A failed
cleanup, worker unwind or lost completion channel leaves sticky **unverified**
state, also after Keep editing or reconnect. That state blocks normal exit and
retains bounded first/latest error details; there is no force-exit control in this
change. Invalidated draft approval returns to editing and must be approved anew.

## What a receipt proves

Typed client construction failures distinguish a locally proven no-outstanding-
child boundary from successful cleanup observation and unverified setup/cleanup.
The worker reports its local ownership outcome independently of its generation-
filtered UI events. Registration happens before worker startup; thread creation
failure retains unverified ownership via the captured guard, and unwind/channel
loss also fails closed.

The production direct-child grace remains two seconds, and existing worker/client
cleanup observation remains three seconds. Scheduling and OS calls can take
longer; neither interval is proof of reaping. No new PID scanning, killing by PID,
subreaper, transport thread, backend protocol or remote cleanup operation is added.
For SSH, success covers only the directly owned local SSH client. It does not
prove the remote agent, descendants, inherited pipes or language server exited,
and detached pipe-reader threads are not joined by this barrier.

Legacy String-returning client APIs retain their historical failure timing; only
the new detailed APIs add explicit postspawn failure observation. See
[connection cancellation](CONNECTION_CANCELLATION.md) and
[explicit Disconnect](EXPLICIT_DISCONNECT.md).
