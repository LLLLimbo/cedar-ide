# Verification report · Explicit idle Disconnect / 0.34.0

The previous [0.33.1 checkpoint](TEST_REPORT_PHASE33_MAVEN_EXIT.md) is fully verified.
This change adds deliberate idle connection release while retaining open drafts.
It introduces no protocol capability or remote cleanup claim.

## Required contract

Disconnect refuses active or unknown command outcomes, language startup/running
or unverified cleanup, pending mutations and close/recovery transitions. Already
completed responses retain their ordinary order; admitted Disconnect cancels
read-only adoption and hands ownership to the existing worker. Buffers, full
selections, Undo/Redo, recovery ownership, unknown-save identities and eligible
offline location history remain. Reconnect is explicit and advances the connection
generation only when the user starts a new connection.

A distinct Disconnecting state ends only on the matching worker's terminal close
receipt. EOF, cancelled Reads and mailbox removal do not certify cleanup. The
worker consumes the existing Client close path with its unchanged three-second
observation budget and two-second child grace. Timeout or unwind reports Cleanup
unverified; its warning survives explicit reconnect. These budgets are not an
unconditional bound on synchronous filesystem or operating-system operations.

## Verification status

The locked offline local aggregate passed 1,296 tests across 41 suites, with zero
failures and 39 ignored opt-ins. Eighteen new frontend tests cover strict admission,
current/stale terminal outcomes, reconnect/close guards, recovery ownership,
unknown-save identity, full selection/Undo, actual disabled tooltip and click
behavior, and queued valid/malformed save acknowledgements before the same-frame
Disconnect handler. Four worker terminal-notice tests cover single completion,
failed establishment, unwind order and a dropped result receiver. Strict Clippy
passed on the host and Windows MSVC target.

Three explicit Linux process cases passed using a normal agent and controlled
stdio peers. Two normal-agent connections were reaped, drafts and recovery were
retained, and no Write/Run/replay occurred. A stalled Read was cancelled without
sending its queued List. A real 20-millisecond test-only close observation timed
out before the unchanged reaper completed; the app retained Cleanup unverified
through replacement. Its later peer-exit observation is reported separately and
never upgrades the failed close receipt to verified cleanup. The six retained
worker process cases, passive-loss case and disk-merge process case also passed,
including completed-write-acknowledgement-before-EOF order.

The response probe reads actual egui state inside its current production frame;
reading it after end_pass had selected a prior disabled Window sizing pass. This
was a test-observation correction, not a production admission change. A test
module was also given an unambiguous filter name so it cannot accidentally select
the separate passive-idle suite. Earlier local failed logs remain preserved.

Both all-feature and default shipping release builds passed. The three explicit
process cases also passed using the newly built default shipping agent. Bundle
inventory/link regressions ran 36 cases: 35 passed and one platform skip. Formatting
and diff checks passed. Independent lifecycle and recovery/test reviews found no
blocking issue after narrowing the ownership-error documentation.

Exact-source dual-OS native acceptance and the regenerated Windows package remain
pending; all previous required CI and extracted-package gates remain required.

## Limits

A successful local owner close is not proof that SSH-side agents or tasks exited,
that detached reader threads joined, or that a language server stopped gracefully.
No heartbeat, automatic reconnect, command cancellation, replay or new watcher is
introduced. Native Windows GUI, authenticated SSH and remote network-loss behavior
remain unverified. See [usage and ownership limits](EXPLICIT_DISCONNECT.md).
