# Workspace backend

`Workspace::open(root)` canonicalizes an existing directory. `handle(Operation)`
implements the shared protocol. Command execution, including Git status, is disabled by default and must
be enabled explicitly with `set_allow_run(true)`.

## Filesystem behavior

- Paths are UTF-8, workspace-relative, and use `/` separators. Empty and `.` paths
  denote the root for listing. Absolute paths, `..`, drive/stream prefixes,
  backslashes, NUL bytes, and every symlink component are rejected.
- Only regular UTF-8 text files up to 1 MiB can be read or saved. NUL-containing
  files are treated as binary. Existing line endings are not normalized.
- Reads return a lowercase SHA-256 revision. Replacing a file requires its current
  revision. `expected_revision: null` means **create only**, with atomic
  no-clobber publication. A stale revision, deleted original, or already-existing
  new file returns `conflict` without changing the destination.
- Replacement is prepared in a same-directory temporary file, inherits existing
  permissions, is flushed/synced, rechecks the revision, and is atomically renamed.
  Read-only files are not replaced. Hard links to the former inode are unaffected.
- These checks are not an OS sandbox or an atomic compare-and-swap with arbitrary
  external writers. A hostile process can rename ancestors between checks; an
  external write in the final revision-check/rename window can race a save. Use a
  trusted workspace and account/container isolation for untrusted workloads.

## Bounded operations

- Listings: up to 4,096 entries and 512 KiB aggregate path/name text. Oversized
  directories return `directory_too_large`, rather than a partial listing.
- Search: literal, case-sensitive, line-based, one-based line numbers; up to 1,000
  matches, 20,000 examined entries, 32 MiB read, depth 64, and five seconds.
  Previews are at most about 2 KiB each, with a 512 KiB aggregate response budget.
  Reaching a bound or encountering unreadable content sets `truncated`.
- Search excludes symlinks, nonregular/oversized/binary/non-UTF-8 files, and
  `.git`, `target`, `build`, `dist`, `node_modules`, `.idea`, `.venv`, `venv`, and
  `__pycache__` directories. It does not interpret `.gitignore`.
- Commands: explicit executable and argument vector, null stdin, workspace cwd,
  no implicit shell. Timeout must be 1–300 seconds. Up to 256 arguments / 64 KiB
  argument text; stdout and stderr each retain at most 256 KiB. Output cap stops
  the command and sets `truncated`; timeout sets `timed_out`. Invalid UTF-8 output
  is displayed lossily. Stderr is captured independently, avoiding pipe deadlock.
- Linux/macOS: each command gets its own process group, killed on timeout/output
  cap and after its leader exits. `waitid(WNOWAIT)` observes termination without
  reaping the leader until after group cleanup, preventing PID-reuse signaling.
  Nonblocking pipe readers have bounded drain/cancellation. Callers must not
  install a competing SIGCHLD reaper or `SA_NOCLDWAIT`.
  A program that deliberately detaches into another session can escape this group.
- Other platforms, including Windows: legacy synchronous command execution and Git status
  return `unsupported_platform`, even with execution trust enabled. Each needs
  a verified containment implementation; Windows requires job-object process-tree
  containment and cancellable pipe capture. The Windows frontend can still use a
  Linux/macOS workspace agent over SSH.
- Git status requires execution trust (`allow_run`): repository-configured clean
  or process filters can execute code during status, even with fsmonitor disabled.
  It uses the system `git` program, a ten-second deadline, bounded output, optional
  locks disabled, fsmonitor disabled, and submodules ignored. Those flags are
  operational protections, not a substitute for trusting repository code.

Filesystem operations do not invoke subprocesses. Subprocess features require
execution trust, including Git status, commands, and language-server support.
Enabled programs have full account permissions and can access paths outside
the workspace.

## Errors

Stable error codes include `invalid_path`, `not_found`, `not_directory`,
`permission_denied`, `conflict`, `file_too_large`, `invalid_utf8`, `binary_file`,
`directory_too_large`, `invalid_query`, `invalid_limit`, `run_disabled`,
`invalid_command`, `invalid_timeout`, `command_failed`, `command_timeout`,
`output_limit`, `git_error`, `unsupported_platform`, and `io_error`. Messages provide diagnostic detail;
clients should branch on codes rather than message text.

## Explicit Git changes and selected-file diff

`GitChanges` and `GitDiff` are additive capability-gated operations. They require
execution trust before inspecting either the executable or repository. Linux and
macOS support both host modes; Windows supports an isolated agent only. Hello
advertises implementation support without probing tools. Legacy `GitStatus` is
unchanged. Select an existing absolute Git executable on the workspace host
(native `.exe` on Windows); Git 2.45 or later with the actual `--no-lazy-fetch`
option is required. Windows currently accepts local-drive paths only.

Only a workspace-root ordinary non-bare repository with a real `.git` directory
is supported. Linked worktrees, shared commondir repositories, object alternates,
ancestor discovery, and symlink/reparse metadata are rejected. Inspection visits
at most 32,768 metadata entries and never follows metadata links. A request has
one ten-second deadline covering version/layout preflights, fresh status, and
diff, plus owned-process cleanup. Each output stream is bounded to 256 KiB;
status is bounded to 4,096 entries and 4,096 bytes per path. Malformed, truncated
or non-UTF-8 output fails, rather than appearing clean or successfully partial.

Status uses strict NUL-delimited porcelain v2. Rename detection is disabled, so
a rename appears as add/delete. Untracked files, conflicts, submodules, symlinks,
and unsupported paths have no diff action. Selected staged/unstaged diff first
rechecks exact eligibility from fresh status within the same deadline. Deleted
parents are supported, while lexical traversal, `.git`, existing directories,
symlinks and Windows reparse points are rejected. Cached diffs support unborn
branches; binary files receive Git's ordinary summary, never a forced text or
binary patch. These actions view disk and index, not unsaved editor buffers;
they do not stage, write, reset, fetch, apply patches or adopt editor baselines.

Every subprocess receives a private environment with inherited `GIT_*` names
removed case-insensitively using native ordinal Unicode comparison on Windows,
while retained OS strings remain unchanged. Global/system configuration and system/global
attributes are suppressed; locale, literal paths, no replace objects, no lazy
fetch, no optional locks and no prompting are fixed. fsmonitor, untracked cache,
diff index refresh, hooks, external diff and textconv are disabled. Suppressing
global/system configuration intentionally can change normal CLI behavior, such
as global `autocrlf` or filter settings. Repository clean/process filters can
still execute, write and access the network with full account authority. These
read-only actions and path checks are operational protections, not a sandbox
against trusted filters or concurrent filesystem changes.

On Windows the handler owns a separate kill-on-close job, bounded capture and
exact process wait. It terminates descendants after root exit or a control
limit, joins pending pipe I/O, and finishes ownership before publishing a result.
It does not use or interfere with user Run tasks or Java language sessions.
