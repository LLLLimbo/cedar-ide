# Architecture · checkpoint 7B / 0.7.0

Cedar is a native Rust frontend plus a workspace backend. The current wire
protocol remains **4**. The handshake requires an exact protocol match, not an
application-version match. Bounded operation capabilities are discovered within
that protocol; unsupported protocol versions are still rejected. Checkpoint 7B
reuses the existing asynchronous-task operations for an isolated Windows agent.
The frontend has no embedded browser or JVM. Language servers and build tools
can still need a JVM or other runtime on the workspace machine.

## Execution and ownership

The native UI owns draft text, tab identity, saved/base revisions, bounded undo
history, and connection generation. Connection and workspace requests execute
on a dedicated worker. Results carry a generation and request ID, so a stale
connection cannot mutate the newly selected workspace. A typed `WorkspaceKey`
keeps local roots and SSH host/port/root/agent fields separate instead of joining
them with delimiters that valid paths or IPv6 hosts can contain. Dirty drafts
are checked before connecting and after Hello; a matching canonical root alone
cannot authorize another endpoint to inherit them. Trust-only changes keep the
same identity, while canonical-root changes still reject dirty reconnects.
Seven no-network regressions cover these boundaries. Save acknowledgements
refer to the exact submitted snapshot and never replace newer typing.

`cedar-client` selects an in-process `Workspace` for non-Windows local files,
the exact bundled sibling agent for Windows Local, or an SSH child-process
transport. Windows Local has no PATH/cwd/environment or in-process fallback.
SSH starts the same `cedar-agent --root ...` exercised by integration tests. The agent remains a sequential, bounded-frame stdio server;
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
[historical hotfix report](TEST_REPORT_HOTFIX_0_3_1.md).
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
Windows asynchronous tasks require the isolated agent host and owned Job/pipe
implementation; in-process tasks, Git, synchronous Run and LSP remain disabled. See [RUN_TASKS.md](RUN_TASKS.md).

## Explicit workspace task profiles

`cedar-app::task_profiles` parses and encodes version-1 `cedar.tasks.json`; it
never runs commands. The root configuration is an ordinary `Document`, so local
and SSH backends share reads, SHA revision checks, undo, save acknowledgements
and frontend-local recovery. Explicit Load uses an already-open editor buffer,
including its unsaved text. Only a genuine `not_found` produces a new unsaved
configuration with a no-clobber create revision. Invalid text is retained for
inspection instead of being replaced by defaults.

The strict schema allows at most 32 uniquely named profiles in 256 KiB encoded
UTF-8. Each stores only name, program, literal ordered arguments and timeout.
Unknown/duplicate fields, positional struct arrays, unsupported versions and
invalid bounds fail before mutation. Executable whitespace, empty arguments,
Unicode and shell-looking bytes remain literal. Output is two-space-indented
JSON with one newline, and its size is bounded independently of input: a valid
compact input can be too large to save prettily without invalidating its draft.
No trust, credentials, environment, working-directory overrides, shell strings,
variable expansion, autorun or dependency graph is stored.

The structured form snapshots workspace identity, connection generation,
document ID, edit version and base revision. Save profile checks that source,
serializes the complete configuration, applies one editor undo transaction and
requests an ordinary save by document ID without switching the active tab.
Same-frame editor input precedes queued profile/close actions. A raw-editor
change, closed tab, stale response or changed connection blocks stale Save/Run;
new typing survives earlier save acknowledgements. Form-only changes participate
in dirty-close/workspace-switch guards but remain session-only until serialized.

Load, selection, creation, Save, Discard, restore and reconnect never dispatch
`RunStart`. Only explicit Run does, and it never saves automatically. A same-
workspace reconnect requires explicit retained-draft review or Load, retaining
the original SHA for later conflict detection. Different workspace identities
do not inherit profiles. Trust is independently granted to the connection and
is never recovered from configuration. Frontend controls require the complete
advertised task lifecycle and separate connection trust. The backend additionally
requires the immutable Windows IsolatedAgent host mode; peer fields cannot grant
it. Synthetic gate coverage does not establish real Windows-to-Linux SSH
interoperability or native GUI acceptance.
See [TASK_PROFILES.md](TASK_PROFILES.md).

## Process transport ownership and fault bounds

The child transport uses one-slot request and response queues and retains only
a 4 KiB stderr tail while draining the pipe. Oversized/malformed/truncated frames,
wrong response IDs, EOF and request deadlines poison the session. Matching
application errors remain ordinary responses and leave the connection usable.
An acknowledgement can be lost after a mutation committed; reconnect and inspect
before retrying. There is no automatic write/command replay or task adoption.
The public deadlines remain 30 seconds for ordinary requests, 75 seconds for
language startup, and bounded requested duration plus ten seconds for legacy
synchronous Run; short fault-test deadlines use private seams.

A dedicated owner/reaper starts when the child is created. Close/failure drops
the request sender and response receiver and signals that owner without waiting
on the caller/UI thread. Once the writer is idle it closes child stdin. The
owner allows two seconds for orderly exit, then attempts to kill/reap only its
direct child. Releasing the response receiver also unblocks a reader stalled on
the full response slot. Exclusive wait ownership is required; a non-interrupted
wait error abandons signaling rather than trusting a potentially reused PID.

This bounds the graceful opportunity, not every operating-system failure.
Escaped descendants retaining pipes, uninterruptible processes and abrupt
frontend death can prevent normal completion. The reaper does not chase arbitrary
PIDs or prove remote cleanup. Linux compiled-agent tests show ordinary EOF and
serve errors unwind the workspace/task owner; a separate agent-SIGKILL test
shows its task can survive until the fixture's own lifetime ends. Real SSH
interruption must be observed separately. See [REMOTE_VALIDATION.md](REMOTE_VALIDATION.md).

## Immutable handshake and backend capabilities

Protocol 4 Hello now optionally includes schema-1 AgentInfo. Workspace reports
compiled implementation support independently of execution trust and installed
tools. Client validates one handshake, caches its entire root/metadata payload,
and rejects missing capabilities locally before transmission. Legacy peers keep
four file operations; advanced tools require an upgraded metadata-aware agent.
No server session token, task adoption or automatic retry is introduced.

The frontend installs this snapshot only after the active generation, typed
workspace identity and canonical-root/recovery checks pass. Both successful and
failed duplicate Hello events are ignored outside an active connection attempt.
Backend capabilities replace frontend-OS/SSH heuristics; readiness, trust and
operation lifecycle groups remain separate conditions. Missing Write blocks
profile Save before raw-document mutation. Optional language features still
intersect agent support with language-server capabilities.

See [REMOTE_CAPABILITIES.md](REMOTE_CAPABILITIES.md) for the wire bounds,
intentional legacy policy change, platform boundaries and regression matrix.

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
flushes data and prepares attributes before its final path/revision verification,
then commits immediately via replace or no-clobber create. Windows ordinary-file
replacement now uses Rust 1.99 `std::fs::rename`, after clearing the temporary-file
attribute and restoring cleanup ownership; delete-sharing readers may keep old
complete bytes while fresh opens see the replacement. Read-only or deny-delete-
sharing restrictions still fail without deleting the destination or retrying.
New-file creation retains `persist_noclobber`. Deterministic preparation hooks
exercise late edits/removals/creation and cleanup; actual Windows runtime proof
for the new path is still pending. A directory writer can race canonicalization
or the final revision check; this is not filesystem-level compare-and-swap,
universal crash durability or adversarial isolation. See
[WORKSPACE_SAVE.md](WORKSPACE_SAVE.md).

OpenSSH owns authentication and encrypted transport. The client requires
existing known-host trust, noninteractive BatchMode and explicit
StrictHostKeyChecking=yes, including localhost. It disables automatic host-key/IP
recording, adding keys to the authentication agent, delegated GSSAPI credentials,
local commands, multiplexing, tunnel/agent/X11/port forwarding. It overrides
inherited settings that would detach SSH, close stdin or suppress the command
session. Only the three OpenSSH-8.7 aliases StdinNull, SessionType and
ForkAfterAuthentication are in IgnoreUnknown; security settings are never
ignored. The option design requires OpenSSH 7.6 or newer, with older-client
runtime acceptance still pending. User-controlled ProxyJump/ProxyCommand routing
remains supported, so SSH configuration itself is trusted code, not a sandbox.
Cedar does not manage secrets. The remote command is quoted for a POSIX shell,
and destination values cannot inject local SSH options. Real stdio subprocess
integration and OpenSSH 10.0p2 local option parsing are tested; authenticated SSH
and real network-failure interoperability are not. No authentication fixtures
have been created; the proposed narrow test remains permission-gated.

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
status is separate in the [phase-4 report](TEST_REPORT_PHASE4.md); these are
historical language-feature results, not phase-5 native retests.

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

## Windows ownership and isolated tasks (7A / 7B)

The independent cedar-winprocess crate supplies atomic suspended Job creation,
owned stdio capture, completed overlapped cancellation and literal UTF-16 argv
encoding. Its 0.6.2 primitive passed actual same-commit Windows CI, including all
13 lifecycle tests. Checkpoint 7B adds a shared task controller with separate
Unix and Windows executors; the Windows owner stays on the supervisor thread and
is destroyed before the terminal record is published.

Windows Local launches only its exact bundled sibling agent. BackendMode defaults
to InProcess, which still rejects Windows tasks; only agent host code selects
IsolatedAgent. Trust remains independent. Native programs require absolute UTF-8
.exe paths, with no shell or PATH/PATHEXT fallback. Native unsigned exit codes
use an optional additive snapshot field while old readers retain compatibility.

Exact HANDLE_LIST does not constrain unrelated broad-inheritance spawns. Windows
Git, synchronous Run and LSP therefore stay unreachable at their backend guards.
They, DAP and future PTY integration require separate ownership design. The new
agent-task and Local-bundle paths need their own exact-commit CI; primitive
acceptance is not integration acceptance. See [WINDOWS_PROCESSES.md](WINDOWS_PROCESSES.md).
