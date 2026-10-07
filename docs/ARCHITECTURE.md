# Architecture · phase 4 / 0.4.0

Cedar is a native Rust frontend plus a workspace backend. The current wire
protocol is **4**; incompatible frontend/agent versions fail the handshake.
The frontend has no embedded browser or JVM. Language servers and build tools
can still need a JVM or other runtime on the workspace machine.

## Execution and ownership

The native UI owns draft text, tab identity, saved/base revisions, bounded undo
history, and connection generation. Connection and workspace requests execute
on a dedicated worker. Results carry a generation and request ID, so a stale
connection cannot mutate the newly selected workspace. Save acknowledgements
refer to the exact submitted snapshot and never replace newer typing.

`cedar-client` selects an in-process `Workspace` for local files or a child-process
transport for SSH. SSH starts the same `cedar-agent --root ...` exercised by
integration tests. The agent remains a sequential, bounded-frame stdio server;
it does not open a listening socket. Each workspace owns one optional language
server and a lazily created asynchronous command manager. File operations,
language-server filesystem access and command execution stay on that machine.

`cedar-recovery` is deliberately frontend-local and independent of that worker.
A dedicated recovery actor performs disk I/O while coalescing pending snapshots.
It never opens project files, authenticates to SSH, starts an agent or grants
execution trust. A remote draft's backup is on the frontend computer, not its
remote workspace host.

## Private recovery and exact acknowledgement

Recovery is on at the start of each frontend session. A one-second trailing
debounce coalesces each typed workspace identity / relative path. The actor
retains at most 64 pending keys and 128 MiB pending text plus one in-flight
operation. The store allows 128 records within a 256 MiB budget including
replacement headroom, with a 1 MiB draft and 1 MiB saved/base text per record.
Oversized text is not silently truncated. Quota, lock, integrity and I/O problems
remain visible; ordinary editing stays available and no old draft is evicted.

A record preserves exact UTF-8 draft/base text, the original optional saved
revision, workspace identity, path and timestamp. The versioned format checks
metadata and payload digests. Startup listing reads bounded metadata only;
explicit review reads and validates the selected payload. A locked single
writer uses monotonic, per-key operation ordering and removal tombstones so a
late queued write cannot resurrect a discarded copy. The creator process owns
the store; inherited use fails closed and only that process explicitly unlocks
on drop. This avoids a transient Unix fork/exec descriptor retaining a dropped
owner's lock without unlocking a still-live parent or a later independent owner.

Writes use a private same-directory temporary, file flush and atomic replace;
Unix additionally synchronizes the containing directory and accepted ancestors.
Only an applied storage mutation matching the latest operation, document,
edit and base revision can show **Draft backed up locally**. Queueing, debounce
expiry, stale completion, failed final sync and merely reading a prior copy are
not acknowledgements for the current draft. Typing, saving, removal or actor
replacement invalidates older acknowledgements.

Restore requires review and a separate **Connect with trust off and restore**
choice. The endpoint/root/agent identity is shown, and the canonical root must
match. Restore neither overwrites an existing tab nor writes the project; it
keeps the original revision so a later save still detects external changes.
Unreviewed older recovery is protected from unrelated fresh disk tabs. Save or
discard cleans only records owned by that tab. Discard-and-quit waits for those
removal acknowledgements; typing during the wait cancels the close. The final
viewport close is emitted after the full input frame.

The plaintext store can contain sensitive source. It omits credentials, trust,
command history and process handles. Unix private permissions and no-follow
checks are defense in depth; Windows inherits ACLs without equivalent privacy
verification. Neither path is a hostile same-account filesystem sandbox.
Unacknowledged input can be lost on process exit. Linux process-kill recovery is
tested; power loss is not. Windows has no equivalent directory-flush guarantee.
Windows core recovery tests have run in external CI and exposed a concurrent-
reader replacement failure. The fix uses Rust 1.99 standard-library rename for
compatible readers; restrictive sharing remains an explicit error with the old
record preserved. The corrected phase-3.1 public commit passed both Linux and
Windows CI, including real Windows recovery tests; exact scope is recorded in
[TEST_REPORT.md](TEST_REPORT.md).
Windows native recovery/privacy acceptance and macOS runtime remain unvalidated. See
[RECOVERY.md](RECOVERY.md) and the [frontend contract](../crates/app/RECOVERY_AND_COMMANDS.md).

## Asynchronous command tasks

Protocol 3 introduced `RunStart`, `RunPoll` and `RunCancel`. `RunStart` accepts work and
returns a task snapshot; it does not mean spawn succeeded. `cedar-tasks` owns a
single supervisor with one active task and eight bounded terminal records. IDs
are process-wide monotonic counters, not child PIDs, and are scoped to their
manager/session. No public API accepts arbitrary PIDs for signaling.

Each task uses explicit executable + literal argv, null stdin, a 1–300 second
UI timeout, and independent 256 KiB raw stdout/stderr caps. Polling replaces a
full bounded snapshot rather than appending repeated output. The UI polls no
faster than every 250 ms while active. The agent can service files and LSP
between task requests; it does not hold a sequential request open until the
command exits. The legacy synchronous `Run`, Git and language/debug processes
are outside this asynchronous scheduler and its one-active-task limit.

Starting, Running and Cancelling are nonterminal. Succeeded, Failed, Cancelled,
Timed out, Output limit and Spawn failed are distinct terminal outcomes. Cancel
is a request, not proof of termination: already observed natural completion can
win the race. A start or cancellation with unknown outcome is never retried
automatically. Task epochs plus connection generations reject late results;
reconnection clears IDs rather than reusing them in a new agent session.

Linux/macOS use nonblocking pipe reads on the supervisor. `waitid` with
`WNOWAIT` observes exit without reaping: the owned group is signaled while the
leader PID is reserved, then the leader is reaped. This prevents signaling a
reused PID and cleans ordinary same-group descendants even after natural
leader exit. Lost wait ownership fails closed without signaling a cached PID.
The final drain is bounded; escaped descendants and other malicious behavior
still require OS isolation. Manager drop waits for ordinary owned-child cleanup,
not an OS-enforced shutdown deadline. Linux is runtime-tested, macOS is not.
Windows local execution remains unsupported pending Job Object containment and
cancellable pipes. See [RUN_TASKS.md](RUN_TASKS.md).

## Failure semantics

- Transport errors close the session; a remote application error such as `conflict` keeps it usable
- A timed-out write has an unknown outcome. Reconnect/read before retrying; revision checking prevents blind overwrite
- Reconnect is explicit and preserves drafts and old disk revisions, intentionally allowing a later save to detect remote changes
- Switching workspaces checks dirty state both before connection starts and when the new connection completes
- Active commands guard close/reconnect with Cancel-and-wait. After terminal status the user retries the intended action, which reruns dirty-file/pending-write protections
- Unknown task outcome warns the command may still run and requires explicit acknowledgement before close/reconnect; it never implies successful cancellation or restarts the command
- Recovery errors do not disable editing or replace dirty-close protection; orderly actor shutdown flushes queued work, but unexpected exit can lose unacknowledged edits
- Agent stdout is protocol only. Process diagnostics go to stderr and the client retains a bounded tail
- Unexpected / oversized / truncated wire frames fail closed

## Security model

This is a developer tool, not a multi-tenant sandbox. A trusted workspace and
its toolchain may execute account-level code only after the user enables tool
execution. Git clean/process filters also make GitStatus subject to this gate.
Language-server `workspace/applyEdit` requests and returned arbitrary commands
are not executed implicitly. Restoring text does not restore trust.

The filesystem layer rejects traversal, absolute paths, platform prefixes,
special files and symbolic links. Project save uses a same-parent temporary,
fsync, late revision verification and atomic replace or no-clobber create.
A directory writer can race canonicalization or the final version check;
this is not filesystem-level compare-and-swap or adversarial isolation.

OpenSSH owns authentication and encrypted transport. The client requires
existing known-host trust, noninteractive BatchMode and explicit
StrictHostKeyChecking=yes. It disables automatic host-key updates, local
commands, multiplexing, agent/X11 forwarding and port forwards. It does not
manage secrets. The remote command is quoted for a POSIX shell, and destination
values cannot inject local SSH options. Real stdio subprocess integration is
tested; authenticated SSH and real network-failure interoperability are not.

## Language, debugging and extensibility

Workspace operations belong in `cedar-protocol` and `Workspace`, sharing local
and SSH transports. Shipping incompatible changes requires a protocol bump;
feature negotiation and general request cancellation remain future work.

LSP has separately bounded frames, outbound messages, pending requests and
events. Notifications never block response routing; overflow is explicit.
UTF-16 positions and full/incremental change modes are tested. The UI schedules
coalesced 350 ms synchronization and bounded event polling, retains diagnostic
version/staleness, resolves workspace URIs on the agent, and validates complete
completion/import plans before a single draft/undo transaction. Connection,
server-session, document and edit-version stamps reject stale results.

`cedar-debugger` separately uses DAP request_seq / command correlation,
independent events and asynchronous launch handles. Phase-2 real Python probes
are historical evidence, not GUI/agent integration. Listener security and
general descendant cleanup remain blockers; DAP does not reuse JSON-RPC routing.
See [DEBUGGING.md](DEBUGGING.md).

## Formatting and language-navigation transactions

Protocol 4 adds `LanguageFormat`, `LanguageReferences` and
`LanguageDocumentSymbols`. The agent keeps the version and byte count of each
synchronized document. Formatting carries the expected version in Cedar's
protocol; the agent rejects a mismatch before contacting the server. That
version is not an invented field on the LSP formatting request. Static provider
capabilities, open-document state, trust and workspace-relative paths gate all
three operations. Results remain inert JSON until the frontend validates them.

Formatting synchronizes the current draft, captures connection generation,
server session, request sequence, document identity/path, edit version and source
text, and builds a complete plain-TextEdit plan. Before/After is read-only.
Apply rechecks the snapshot and changes only draft text through one native undo
transaction; it neither writes the project nor changes the saved baseline or
revision. Recovery observes the resulting draft through its ordinary ownership
rules, without gaining authority over an older unowned copy. Null, empty or
text-identical results are no-ops. Cancel, Escape and closing the preview discard
the proposal. A shared planner rejects unsupported shapes, overlaps, ambiguous
insertions, invalid UTF-16/CRLF positions and count/byte excess before mutation.

Changing text, switching tabs (even away and back), close/reopen, reconnect,
server restart or newer feature requests invalidate old proposals. Cursor-only
movement is permitted for document formatting and outline; Apply maps the latest
cursor through the validated edits. Outline selection supersedes older pending
URI resolutions and file opens, so a late reference response cannot steal focus.
Editor Undo/Redo skips same-text cursor checkpoints for shortcut-only batches,
while mixed text/paste/navigation batches pass unchanged to egui to preserve input
order. History is bounded and cloned only for deliberate edits/history actions.
Full-frame regressions cover navigation and mixed input; native release acceptance
status is separate in [TEST_REPORT.md](TEST_REPORT.md).

References synchronize every matching open draft before dispatch and capture
each participating document's identity, text, edit and acknowledged LSP version.
A changed participant set, draft or source query position invalidates the pending
result. Returned locations have no target versions: the UI labels retained
results an **unversioned server snapshot**, with no freshness promise for unopened
files or targets changed afterward. Reference and flat-outline URIs are resolved
by `LanguageResolveUri` on the agent before opening. Existing dirty target tabs
are reused unchanged, and each selected range is validated against current text.

Outline is an explicit refresh, not a live index. Homogeneous `DocumentSymbol[]`
keeps hierarchy and navigates with `selectionRange`; `SymbolInformation[]` stays
flat and displays its container as context. Hierarchical ranges require valid
UTF-16 boundaries, selection containment and child/parent containment. Edits
invalidate the outline. Mixed/hybrid shapes are rejected and unknown numeric
symbol kinds use a generic label.

Limits include 1,024 plain edits and independently 1 MiB source/result/inserted
text, 1,024 reference locations with 16 KiB per URI and 512 KiB aggregate URI text,
and 2,000 outline nodes with depth 32 and 512 KiB retained text. See the
[edit planner](TEXT_EDITS.md) and
[frontend contract](../crates/app/PHASE4_LANGUAGE.md) for exact inclusivity and
failure behavior. These do not authorize commands, server-originated edits,
WorkspaceEdit or resource operations.

Multi-file rename remains deferred: unversioned cross-file responses need
trustworthy source snapshots, cross-document atomic undo and explicit resource
semantics. A server may omit required file renames when the client does not
advertise them, so valid text edits alone cannot establish a complete rename.
See [REFACTORING_ROADMAP.md](REFACTORING_ROADMAP.md). A future plugin host should
use capability-scoped out-of-process RPC, not arbitrary native libraries in
the UI. Project models, build tools and adapters should remain off the UI thread
and preferably agent-side. No IntelliJ plugin or complete-feature compatibility
is promised.
