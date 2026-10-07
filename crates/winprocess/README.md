# Cedar Windows process ownership primitives

Checkpoint 7A added an independently testable Windows-only owner and a portable
literal-argv encoder; revision 0.6.2 passed actual Windows primitive CI, including
all 13 lifecycle tests. Checkpoint **7B / 0.7.0** adopts it for asynchronous tasks
in the isolated agent only, and adds CREATE_NO_WINDOW for non-interactive console
children. Git, legacy synchronous Run and language execution remain disabled on
Windows. Nothing in this crate grants workspace trust. Task integration passed
all thirteen primitive lifecycle and seven agent/bundle cases at 0.7.1 and 0.8.0.
Checkpoint 9A adds optional piped stdin; its new behavior requires independent
exact-commit Windows runtime acceptance before LSP integration.

## Supported host and launch policy

The host must be an isolated process with controlled subprocess creation.
HANDLE_LIST restricts this child; it cannot prevent an unrelated concurrent
broad-inheritance spawn from taking temporary inheritable stdio handles.
Do not embed this launcher in the GUI process or claim a task-only mutex solves
third-party spawning. The existing task integration uses the bundled agent; future language execution
must preserve the same host constraint.

Windows 10 / Server 2016 or newer is required for atomic JOB_LIST assignment.
There is no unconfined fallback or breakaway request. Native executable and cwd
paths must be absolute, existing, UTF-8 representable paths; the executable must
have an explicit .exe extension. The OS validates its executable image. There is
no PATH/PATHEXT lookup, cwd mutation, file association, batch-file expansion or
implicit shell. Explicit cmd.exe remains a caller-selected shell with its own
parsing rules, not the ordinary C/Rust argv guarantee.

## API and responsibility split

On Windows, `WindowsCommand::spawn_suspended(&LaunchSpec)` owns an already
job-assigned suspended process and its capture handles. `resume` is explicit and
single-use. `try_exit` observes the root only; callers terminate the job after
root exit before completing their finite final-output drain.

- `capture_round` delivers at most four 8 KiB chunks per stream to a temporary
  callback slice; the primitive retains no full output transcript
- The caller owns retained-output caps, deadlines, cancellation reasons and task
  history; this crate does not provide a task scheduler
- `terminate_tree` requests owned-job termination; success alone is not an
  observed exit. `wait_exit` blocks and must stay off a frontend thread
- `cancel_capture_and_complete` stops capture and joins outstanding I/O. It can
  discard a final racing chunk after the caller's drain budget. Cancellation
  does not manufacture a transport EOF
- `observation_handle` duplicates only query/synchronize rights, non-inheritable;
  it grants no process-termination authority
- Native exit codes remain u32, including 259 and high-bit values

Drop terminates the job, cancels and completes capture, waits for the root, then
releases ownership. Components also clean up partial failures and unwinding.
Exceptional stuck OS operations can delay cleanup. Memory still referenced by
kernel I/O is never freed to meet a timer. No detached reader threads are used.

### Piped stdin prerequisite (native acceptance pending)

`spawn_suspended` continues to use NUL stdin. The explicit
`spawn_suspended_with_piped_stdin` variant adds a private outbound overlapped
pipe and a synchronous child reader, with the same job-before-resume and exact
three-handle inheritance contract. This primitive does not enable Windows LSP
or change agent capabilities. Its new code requires real Windows CI before use
in an enabled service; the earlier capture/task checkpoints do not cover it.

- `begin_stdin_write` copies up to `MAX_STDIN_WRITE_BYTES` (64 KiB) into one
  pinned allocation; it never retains the caller's slice. A second operation
  returns `WouldBlock` until the first completion has been observed
- `begin_stdin_write` or `poll_stdin_write` returns `Written(n)` exactly once.
  Short counts retain a caller-owned suffix; zero-byte completion of nonempty
  input is `WriteZero`. Empty input completes locally without an OS null write
- `close_stdin` is idempotent but returns `WouldBlock` while completion remains
  outstanding. It closes the writer for EOF after polling; it never flushes or
  disconnects the pipe. Transport completion is not child acknowledgment
- `cancel_stdin_and_complete` always closes the input after joining I/O. It
  returns `Written(n)` if completion won the race, `Cancelled` if cancellation
  completed, or `Idle` if there was no pending operation. Cancellation and any
  write error may follow partial transmission; abandon a framed connection
  instead of replaying its message. `Cancelled` has no reliable delivered count
- NUL/closed stdin polls as `Closed` and rejects writes with `BrokenPipe`

One fixed 64-KiB user buffer, one event and one parent pipe handle are added per
piped owner. There is no write queue, `std::io::Write`, implicit frame buffering
or detached thread. The kernel's pipe buffer request is also 64 KiB, but Windows
documents that reservation as advisory; it is not an exact kernel-memory cap.
The caller owns protocol frame bounds, short-write continuation, read/write
fairness, deadlines and connection poisoning after partial transmission.

Drop still requests job termination first, then cancels/completes stdin and
both captures, waits for the root, and releases the owned handles. An unexpected
stdin cleanup error does not skip capture cleanup. OS cancellation completion
can delay teardown without a fixed worst-case deadline; live OVERLAPPED or
buffer storage is never released to satisfy a timer.

## Safety boundaries

- KILL_ON_JOB_CLOSE is set before creation. JOB_LIST assigns at creation rather
  than after spawning, avoiding an unowned suspended-child crash window
- Exact stdio HANDLE_LIST; process/thread/job/reader handles are non-inheritable
- Attribute values are boxed and survive until the aligned attribute list is
  deleted. Pipe buffers and OVERLAPPED state are pinned and use UnsafeCell for
  kernel writes; completion precedes observation or reuse
- Pipes are first-instance, local-only, and protected by a current-logon DACL.
  A random name prevents collisions but is not authentication; the connected
  client is verified as the creating process before writer inheritance
- No account grants, stored credentials, global ACL/network changes, PID-based
  termination, arbitrary process enumeration or security-setting changes
- A trusted launched program still runs with account-level privileges. This is
  ownership/cleanup infrastructure, not a hostile-code sandbox

More rationale and official contracts: [Windows process design](../../docs/WINDOWS_PROCESSES.md).

## Tests

Ordinary Linux/Windows tests cover the pure encoder and fixture wire format.
Windows-only unit tests exercise jobs, failure/unwind paths, pipe security,
cancellation completion and ownership. The ignored Windows lifecycle suite
requires a separately built, short-lived fixture and serial execution:

```powershell
cargo build -p cedar-winprocess --features fixtures --release --locked
$env:CEDAR_WINPROCESS_FIXTURE_BIN = "$PWD/target/release/cedar-winprocess-fixture.exe"
cargo test -p cedar-winprocess --features fixtures --test windows_lifecycle --locked -- --ignored --test-threads=1
```

The fixture is test-only, capped at five seconds, and is not included in desktop
binary artifacts. It does not discover or run a shell, JDK or user project.
Cross-compilation alone is not lifecycle acceptance; see the current report.
