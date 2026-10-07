# Cedar Windows process ownership primitives

Checkpoint **7A / 0.6.2** adds an independently testable Windows-only owner and a
portable literal-argv encoder. **Cedar's Windows TaskManager, Git and language
execution remain disabled.** Nothing in this crate grants workspace trust.
Windows runtime acceptance is required before any task integration.

## Supported host and launch policy

The host must be an isolated process with controlled subprocess creation.
HANDLE_LIST restricts this child; it cannot prevent an unrelated concurrent
broad-inheritance spawn from taking temporary inheritable stdio handles.
Do not embed this launcher in the GUI process or claim a task-only mutex solves
third-party spawning. The intended next integration uses the bundled agent.

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
