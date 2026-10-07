# Windows process ownership · checkpoints 7A / 7B / 9A

## Scope and staged activation

The standalone `cedar-winprocess` primitive passed actual Windows CI at
[`139320e6bb988eb3de4ceecb992d68cd3d0442dd`](https://github.com/LLLLimbo/cedar-ide/commit/139320e6bb988eb3de4ceecb992d68cd3d0442dd):
39 library tests and all 13 explicitly selected lifecycle tests passed in
[the same-commit Ubuntu/Windows run](https://github.com/LLLLimbo/cedar-ide/actions/runs/37662728974).
That establishes the primitive gate, not acceptance of the new task integration.
The earlier Linux integration timeout did not recur; its original cause remains
unconfirmed after fixture hardening and added diagnostics.

Checkpoint 7B integrates **only asynchronous RunStart/Poll/Cancel** in an isolated
agent. `BackendMode` is an immutable host declaration, not a serialized capability,
CLI switch, workspace setting or source of execution trust. `Workspace::open`
and `TaskManager::new` retain the InProcess default, which rejects Windows tasks.
The agent executable explicitly selects IsolatedAgent in code. Direct task
requests check trust and host support before creating the lazy supervisor;
capability advertisement uses the same host predicate.

Windows Local connects through the exact `cedar-agent.exe` sibling of the
frontend executable. A missing or invalid bundle is an error: there is no PATH,
working-directory, environment override or in-process fallback. The existing
process client owns the agent transport and reaper. Both bundled executables are
required; building only the frontend is insufficient for local Windows editing.
The backend selection does not change draft identity or grant `allow_run`.
The bundled agent and task launcher use CREATE_NO_WINDOW for their stdio-only
console processes, preserving redirected streams. GUI executables may still
show their own windows; this is not a general UI-suppression guarantee. See
[Microsoft process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags).
7B re-runs all primitive tests after this per-child creation-flag change.
The first 7B Windows run observed an additional live Job member for each console
fixture. Microsoft's [console design](https://github.com/microsoft/terminal/blob/main/doc/specs/%23492%20-%20Default%20Terminal/spec.md#inbox-console)
permits a console host even without a visible window; its actual identity was
not queried. Revision 0.7.1 therefore treats known live fixtures as a lower bound
while retaining exact zero-member Job cleanup and each held process observation.
Both suites subsequently passed at 0.7.1 and again at
[0.8.0](https://github.com/LLLLimbo/cedar-ide/actions/runs/37685061425):
all thirteen primitive lifecycle and seven isolated-agent/bundle cases executed.
Checkpoint 9A adds optional cancellable stdin and requires a new exact-commit
run, including its eight additional lifecycle and nine additional Windows units.

Git, legacy synchronous Run, persistent LSP and DAP remain outside this Windows
activation. Their direct backend rejections stay in place, not merely hidden
controls. An unrelated broad-inheritance spawn in the agent could otherwise
inherit a temporary task writer. These services need their own ownership and
cancellable-I/O adoption before they can share this host.

Windows task programs must be explicit absolute UTF-8 paths to native `.exe`
files. There is no PATH/PATHEXT lookup, implicit extension, relative expansion or
batch-file translation. Literal arguments follow the native encoder contract;
an explicitly selected shell still applies that shell's own parsing rules.
The command editor shows this Windows-agent requirement. Unix command lookup
is unchanged.

## Task supervision and exit reporting

The common controller owns one active record, eight completed records, bounded
raw output and cancellation state. A WindowsCommand lives only on the supervisor
thread. It is created suspended with its job assigned, then cancellation/deadline
is checked again before resume. A cancellation racing a subsequent resume may
still briefly execute code; no automatic retry is permitted.

Natural root exit is observed before cancellation/timeout precedence is chosen.
Every terminal path terminates the job, including natural exit with surviving
descendants, then performs a bounded final drain and completes outstanding I/O.
The owner is destroyed before the controller publishes the terminal snapshot.
The usual 256-KiB stream caps, 250-ms drain budget and 300-second task limit remain;
exceptional OS operations can still delay cleanup.

`TaskSnapshot.windows_exit_code` is an additive optional unsigned 32-bit field.
Windows retains every native code there; the legacy signed `exit_code` is filled
only when the value fits i32. It is absent for high-bit Windows codes, not wrapped.
Old protocol-4 readers can ignore the new field; new readers accept old snapshots
without it. Unix serialization is unchanged. The UI prefers the native value and
shows decimal and hexadecimal. Success uses native zero; a signaled process that
returns 259 has completed with a nonzero status. Cancellation/timeout causes stay
separate from the OS termination code.

The current checkpoint still needs its own exact-commit agent-task CI, including
malformed transport, owner death, output limits and the colocated Local bundle.
Primitive CI is not inherited as proof of this new supervisor or frontend route.

## Atomic launch and immutable ownership

The owner creates a non-inheritable unnamed job with KILL_ON_JOB_CLOSE, then
creates a suspended native process using both JOB_LIST and an exact three-handle
stdio list. Membership is verified before success; resume checks the expected
previous suspend count. Unsupported attributes/nested configurations fail
closed. No breakaway or unconfined retry is attempted.

JOB_LIST requires Windows 10 / Server 2016+. Attribute value buffers must outlive
the attribute list, so this implementation keeps handle arrays on stable heap
storage and deletes the correctly aligned list before releasing its values.
See [Microsoft's attribute contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).

Assigning a child only after suspended creation leaves a parent-crash interval
before assignment. Creation-time assignment removes that gap; see
[Microsoft's atomic job guidance](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812).
The runtime still permits legal containing/nested jobs and does not escape CI
containment. See [nested job behavior](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs).

All termination targets are owned job/process handles. Even a failed membership
check independently terminates the newly created root handle, rather than
assuming an empty/wrong job can cover it. A non-inheritable observation duplicate
has only query/synchronize rights. Exit is determined from process signaling
before querying the native u32 code; code 259 is not used as a liveness test.

## Private, completion-owned capture

Two byte pipes have one instance, first-instance exclusivity and remote clients
rejected. Their protected DACL permits the current logon identity, rather than
default Everyone/anonymous read access. Missing or malformed logon identity
fails closed. The random pipe name is collision resistance, not a credential.
Cedar opens both endpoints and verifies its own client PID before making writer
handles inheritable. Same-logon malicious process isolation is not claimed.
[Microsoft pipe access rules](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)

Each reader owns an event and stable pinned storage for OVERLAPPED plus an 8 KiB
buffer. Explicit state distinguishes idle, pending, EOF and stopped capture.
UnsafeCell represents kernel writes while no Rust references to that storage
exist. Exclusive ownership may move between threads; shared access is not
allowed. The primitive creates no reader threads.

Capture performs at most four reads per stream per round and retains no complete
transcript. A caller must impose output retention, timeout and final-drain
budgets. Natural root exit does not itself prove descendants have stopped;
terminate the owned job before finishing the drain.

CancelIoEx is only a request. Completion can win, including ERROR_NOT_FOUND;
GetOverlappedResult establishes completion before buffers/OVERLAPPED are reused
or destroyed. Both streams are cancelled and completed even if one reports an
error. A last racing chunk can be discarded after the caller ends its drain,
but stopping capture is never reported as a real EOF. See
[CancelIoEx](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelioex)
and [ReadFile lifetime requirements](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-readfile).

The composed Drop order is job termination, I/O cancellation/completion, root
wait, then handle release. Failed launch explicitly closes parent writer copies
before capture cleanup. Components separately handle partial initialization and
unwind. Kernel stalls may delay cleanup; freeing live I/O storage is never a
valid deadline workaround. The primitive is not an OS-enforced hard-time sandbox.

## Piped stdin prerequisite (not an LSP activation)

The explicit `WindowsCommand::spawn_suspended_with_piped_stdin` constructor
retains the atomic job and exact three-stdio-handle launch. Ordinary
`spawn_suspended` tasks keep NUL input. An outbound byte pipe gives the parent
an overlapped write endpoint and the child a synchronous reader. Both directions
share the existing current-logon DACL, first-instance exclusivity, local-only
mode, parent-client PID verification and delayed child-end inheritance.

The writer owns one event and one pinned 64-KiB buffer/OVERLAPPED allocation.
It copies the bounded caller chunk before submission and holds no Rust buffer
reference while Windows owns the operation. There is at most one outstanding
write and no queue or thread. Begin and poll return the actual byte count once;
short completion is observable, and zero progress for nonempty input fails with
`WriteZero`. Large frames remain a higher-level responsibility. The 64-KiB pipe
reservation is advisory, as documented by
[CreateNamedPipeW](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-createnamedpipew).

An idle close releases the writer for child EOF. A close with outstanding
completion returns `WouldBlock`, leaving ownership intact. Explicit cancellation
requests `CancelIoEx`, waits for completion even after `ERROR_NOT_FOUND`, and
closes the pipe. It distinguishes no pending I/O, completion winning with an
actual byte count, and `ERROR_OPERATION_ABORTED`. The latter does not prove that
zero bytes reached the child. Every write error is terminal for this pipe;
future LSP framing must poison the connection after partial timeout/error rather
than resend a possibly transmitted frame. Argument-bound and already-pending
rejections occur before submitting new bytes.
[WriteFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-writefile)
and [GetOverlappedResult](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-getoverlappedresult)
define submission and completion; caller buffers are never lifetime-dependent.

No `FlushFileBuffers` is used: on a pipe server it waits for client consumption.
No `DisconnectNamedPipe` is used, because it discards unread pipe data.
Write completion is transport acceptance, not language-server acknowledgment.
See [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)
and [DisconnectNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-disconnectnamedpipe).
Drop requests job termination, joins input I/O, joins both captures even when
input cleanup fails, waits for the root, and finally releases handles. Kernel
completion can delay cleanup; the implementation does not promise a hard bound.

New Windows units exercise real blocked writes, completion/cancel races,
bounded submission, copied-buffer lifetime, immediate rejection after reader
closure, inherited endpoint flags, collision/early-client rejection and partial
connect cleanup. The pending-write round trip mutates and drops the caller's
source while a real write remains pending, then verifies the originally submitted
bytes exactly. It does not control Windows' internal data-copy timing.
The short-count branch has a synthetic completion-count test:
native byte pipes in `PIPE_WAIT` do not provide a deterministic short-success
trigger, so no actual short OS write is claimed. New ignored lifecycle cases
exercise multichunk binary round trips, exact stdout/stderr and EOF, unchanged
NUL behavior, cancellation while a nonreading child is alive, early root exit,
pending-I/O tree Drop, independent owners, sentinel exclusion, owner crash and
repeated failed/suspended/completed handle counts. The existing Windows CI
command selects them with `--ignored --test-threads=1`; Linux excludes the
Windows target entirely. Cross-target clippy proves compilation only. New
same-commit native results are required before any integration or activation.

## Inheritance is a host-wide constraint

HANDLE_LIST protects only the new child. Another broad-inheritance spawn in the
same host could receive a temporary inheritable writer. Rust 1.99's normal
Command path has its own private spawning coordination; a Cedar-only mutex
cannot constrain arbitrary dependencies. The supported integration target is
therefore the isolated, controlled-spawning agent, not the GUI process. Inspect
[the pinned Rust process source](https://raw.githubusercontent.com/rust-lang/rust/1.99.0/library/std/src/sys/process/windows.rs)
when changing this boundary.

## Literal command lines

The encoder treats argv[0] separately and applies Microsoft C/Rust argument rules
to the remaining strings. It preserves empty/Unicode/quote/backslash arguments,
rejects NUL/program quotes, and caps the entire terminated command line at 32,767
UTF-16 units. A non-allocating first pass bounds the second allocation. There is
no implicit shell or normalization. An independent parser and exhaustive short
inputs complement actual Windows fixture round trips.

The executable remains an explicit absolute .exe path; the OS performs image
validation. No partial PE parser is used as a security claim. Future PATH and
workspace-relative resolution must be separately documented/tested. Batch build
wrappers need an explicit shell or native Java launch policy; existing JVM build
daemons are not newly created descendants and are outside this job's ownership.
[Microsoft argument rules](https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments?view=msvc-170)

## Acceptance gates

Local checks cover the portable encoder, fixture format and Windows cross-
compilation. Real Windows CI must additionally run unit and isolated lifecycle
cases: suspended membership/Drop, partial failures, non-unit suspend counts,
owned observation rights, exact argv/cwd, nested descendants, live-writer read
cancellation, output drainage, unrelated-handle exclusion, owner death and handle
counts. Child readiness follows the last fallible pipe write, so broken-pipe
exit cannot masquerade as job cleanup. Crash tests observe the child before any
outer guard cleanup and do not create a driver job that could mask inner failure.

Fixtures are internally capped at five seconds. The serial lifecycle driver has
a per-case watchdog; CI also bounds the Windows primitive step and test job.
These are regression safeguards, not a product termination guarantee. Actual
same-commit Windows results belong in the verification report before activation.
