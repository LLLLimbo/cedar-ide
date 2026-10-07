# Windows process ownership · checkpoint 7A

## Scope and staged activation

`cedar-winprocess` is a standalone primitive crate. Checkpoint 7A deliberately
leaves existing Windows task/Git/language capability predicates and direct
backend rejection unchanged. First validate the primitive on actual Windows;
then a separate checkpoint can integrate asynchronous tasks in an isolated
agent. Git, legacy synchronous Run, persistent LSP stdin and DAP ownership each
need their own adoption and verification.

The next Windows Local design uses the exact bundled sibling cedar-agent.exe,
not PATH/cwd discovery or a fallback to trusted in-process execution. An internal
host mode will distinguish InProcess from IsolatedAgent, never a peer-supplied
protocol field. This migration is **not implemented** in 7A. It must preserve
explicit allow_run and immutable capability/generation handling.

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
