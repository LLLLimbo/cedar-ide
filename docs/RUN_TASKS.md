# Bounded asynchronous command tasks

`cedar-tasks` supplies the Phase 3 command supervisor. The workspace bridge
exposes `RunStart`, `RunPoll`, and `RunCancel`; ordinary file/LSP requests can
continue between polls while an accepted command is running. This is a direct
executable runner, not a shell parser, terminal emulator, job scheduler, or OS
sandbox.

## Trust and invocation

The workspace's explicit `allow_run` trust gate remains authoritative. The
library does not grant trust or discover commands; callers must check permission
before invoking `TaskManager::start`. Executed programs inherit the agent's
account permissions and environment and can access resources outside the
workspace. Setting the working directory does not restrict filesystem access.

The executable and argument vector are passed directly to `std::process::Command`.
Shell substitutions, pipes, globs, and redirection have no special meaning in
ordinary arguments. A caller can explicitly select `/bin/sh` with `-c`, in which
case the caller has deliberately requested shell interpretation. Relative
program/PATH resolution follows the platform's ordinary process-spawn semantics;
use an absolute executable path when its identity must be unambiguous.

Stdin is null. Stdout and stderr are separate, nonblocking, bounded byte streams.
No interactive input, PTY, automatic retry, installation, or privilege change is
performed.

## Public API

```rust,no_run
use cedar_tasks::{TaskManager, TaskState};
use std::time::Duration;

# fn example() -> Result<(), Box<dyn std::error::Error>> {
// Construct lazily after the workspace's execution trust check.
let tasks = TaskManager::new("/trusted/workspace")?;
let id = tasks.start(
    "/usr/bin/git".into(),
    vec!["--version".into()],
    Duration::from_secs(10),
)?;
let snapshot = tasks.poll(id)?;
if !snapshot.state.is_terminal() {
    // Optional explicit user cancellation; this does not wait for process exit.
    let cancellation = tasks.cancel(id)?;
    assert!(cancellation.state == TaskState::Cancelling || cancellation.state.is_terminal());
}
# Ok(())
# }
```

- `new(root)` canonicalizes a directory and starts one idle supervisor thread
- `start(program, args, timeout)` accepts one bounded request and returns an ID;
  process launch happens on the supervisor, so `start` never waits for execution
- `poll(id)` returns the latest full bounded snapshot without consuming output
- `cancel(id)` requests cancellation and returns a snapshot immediately; it is
  idempotent for retained terminal tasks and repeated active requests
- Dropping the manager requests cancellation, joins the supervisor, and reaps
  its normal owned child

A successful `start` means **accepted**, not successfully spawned. Missing or
non-executable programs are reported by a retained `spawn_failed` snapshot with
an error. A command that may have run is never automatically retried. Validation,
busy, expired-task, platform, and supervisor errors use `TaskError`, whose
`code()` method supplies stable bridge error codes.

`TaskId` is a process-wide monotonic `u64`, independent of child PIDs. It never
wraps; exhausting the counter rejects new starts. A different manager cannot
poll or cancel another manager's IDs. IDs expire on history eviction or manager
drop and must not be reused as session identity after an agent restart or new
connection. No API accepts a server-, adapter-, or user-supplied PID to kill.

## States and races

Snapshots contain `id`, `state`, full `stdout`/`stderr`, optional `exit_code`,
`truncated`, and optional `error`. Serialized states use snake_case:

| State | Meaning |
|---|---|
| `starting` | Accepted; supervisor has not finished launching it |
| `running` | Spawned and being supervised |
| `cancelling` | Cancellation requested; terminal outcome is not yet established |
| `succeeded` | Observed normal exit with status zero |
| `failed` | Nonzero/signal exit, or a capture/ownership/supervision failure |
| `cancelled` | Cancellation won before launch or before natural exit was observed |
| `timed_out` | Deadline won before launch or before natural exit was observed |
| `output_limit` | An output stream exceeded its raw-byte cap |
| `spawn_failed` | The executable was not successfully spawned |

Each supervisor turn takes a bounded amount of output and observes exit with
`waitid(WNOWAIT | WNOHANG)`. An already observed output limit or capture failure
wins first. Otherwise observed natural exit wins a simultaneous cancellation or
timeout. If still running, cancellation wins before timeout. Once the supervisor
chooses a termination cause, repeated cancellation cannot overwrite it.

A cancellation request is therefore best-effort and can return `cancelling`
followed by a natural `succeeded`/`failed` outcome. Cancellation before spawn is
consumed prevents launch; a cancellation racing an in-progress spawn may still
briefly execute the program. Neither race justifies silently retrying it.

Final draining can discover an output limit after natural exit; this changes a
natural result to `output_limit`. A limit or read failure found after an already
chosen cancellation/timeout leaves that primary cause intact and sets
`truncated` (and `error` for a read failure). A nonzero process exit alone does
not imply a capture error or truncation. `exit_code` is absent for signal exits,
pre-spawn cancellation/timeouts, and failed spawns.

## Bounds and output handling

| Resource | Limit |
|---|---:|
| Active asynchronous command per manager | 1 |
| Waiting request records | At most that same single active record |
| Retained completed records | 8, oldest evicted first |
| Raw stdout | 256 KiB per task |
| Raw stderr | 256 KiB per task |
| Raw output retained across active + history | At most 4.5 MiB |
| Executable text | 4,096 bytes, nonempty, no NUL |
| Arguments | 256, no NUL, at most 64 KiB combined |
| Timeout | Greater than 0 and at most 300 seconds from acceptance |
| Error text | 4,096 UTF-8 bytes |
| Reads before checking control state | At most four 8 KiB reads per stream |
| Ordinary control check interval | 10 ms, cancellation also wakes the supervisor |
| Final nonblocking drain | 250 ms |

Only the single asynchronous manager's tasks count toward its active limit.
Legacy synchronous `Run`, Git status, and separate language/debug processes are
not part of this task scheduler.

There is no growing message queue, per-read thread, or per-poll history. Completed
records retain one bounded raw capture each. Polling materializes one full
snapshot; callers should replace displayed output instead of appending repeated
snapshots or keeping unlimited snapshots of their own. Extremely aggressive
polling can consume CPU/memory in the caller and should be paced.

Raw bytes are retained internally. UTF-8-lossy conversion happens only when a
snapshot is requested, so a valid character split across reads becomes intact
once its remaining bytes arrive. A live snapshot can temporarily end with a
replacement character; truly invalid bytes and a final incomplete sequence are
presented lossily. Each returned stream string is at most three times its raw
cap due to UTF-8 replacement characters. JSON encoding adds its own bounded
escaping overhead. Exactly reaching the raw limit is allowed; reading an extra
byte marks the stream truncated, stops capture for that stream, and terminates
the command.

## Process ownership and cleanup

Linux is runtime-tested. macOS uses the available Unix process-group, `fcntl`,
and `waitid`/`WNOWAIT` interfaces, but has not been runtime-tested in this
workspace. Windows and other platforms return `unsupported_platform`; Windows
execution remains disabled until Job Object containment and cancellable pipe
handling are verified. Windows frontends can use a supported remote agent.

The child starts in its own process group. The supervisor must exclusively own
its wait state: do not install a competing global SIGCHLD reaper, ignore SIGCHLD,
or enable `SA_NOCLDWAIT`. It observes exit without reaping, signals the owned
group while the unreaped leader still reserves its PID, then waits/reaps. This
ordering is used for natural exit as well as timeout, cancellation, and output
limit, so a leader cannot leave ordinary descendants holding its pipes open.
An uncertain wait-ownership error fails closed without signaling a cached PID.
The owned-child drop guard also handles unwinding without leaving normal children
running. No signal is sent after the child is reaped.

Nonblocking readers run directly on the supervisor; there are no detached reader
threads that can remain stuck in inherited pipes. If a writer still holds a pipe
after normal cleanup, the final drain deadline expires, capture is marked
truncated, and descriptors are dropped.

This is not containment against malicious programs. A descendant can deliberately
create another session/group, daemonize, or pass a pipe to an unrelated process.
Such processes can outlive task completion and manager drop. Kernel-level
uninterruptible operations, a stuck spawn/filesystem, or processes that become
unsignalable through privilege changes can delay supervisor shutdown. `start`,
`poll`, and `cancel` themselves do not wait on those process operations; manager
drop waits for normal owned-child cleanup and is not an OS-enforced deadline.
Use an actual OS sandbox/cgroup/container/account boundary for stronger limits.

## Verification

Run:

```sh
cargo test -p cedar-tasks --locked
cargo clippy -p cedar-tasks --all-targets --locked -- -D warnings
cargo check -p cedar-tasks --all-targets --locked --target x86_64-pc-windows-msvc
```

The Linux suite covers real start/poll/cancel latency, partial live output,
success/nonzero exit, literal arguments and cwd, null stdin, timeout, cancellation,
stdout/stderr floods, leader exit with descendants holding pipes, repeated cancel
versus immediate exit and a newer command, concurrent start rejection, bounded
history and cross-manager IDs, drop cleanup, split Unicode, spawn failure,
validation, and no implicit retry.

Unit tests additionally verify serialization, `WNOWAIT` before reaping, lost wait
ownership, unwind cleanup, a live inherited-writer equivalent with a bounded
nonblocking drain, real read failure, injected interrupted/would-block reads,
and bounded error text. Tests use synthetic commands and temporary directories;
they do not access user servers. macOS runtime and Windows execution are not
claimed as tested.
