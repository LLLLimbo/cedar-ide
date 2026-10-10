# Private draft recovery

`cedar-recovery` stores unsaved normal UTF-8 buffers on the **frontend computer**,
including buffers from SSH workspaces. It does not open a workspace file, connect
to SSH, launch an agent, enable tool execution, or save a recovered buffer into a
project. Restore is an explicit frontend operation which loads one draft into
memory. The ordinary backend save/conflict checks still apply.

## What is retained

A draft contains:

- A typed workspace identity: local root, or SSH host/alias, port, remote root,
  and agent path
- Its exact portable relative path
- The exact UTF-8 draft text and last saved/base text
- The original optional base revision and modified Unix timestamp in milliseconds

A missing base revision denotes a never-saved file. Recovery does **not** refresh
an old revision against today's filesystem. Otherwise, restoration could silently
turn an external-change conflict into permission to overwrite newer work. The
frontend must pass the retained revision to the existing backend write operation;
a new-file save must still reject an already existing destination.

The storage schema has no credentials, execution-trust flags, command history,
process handles, or authentication state. Identity fields are labels and are
never executed. Draft contents can themselves contain private source or secrets;
this is local plaintext storage, **not encryption or a secrets vault**.

## Location and privacy

`default_store_path()` chooses, in order:

1. An absolute `CEDAR_RECOVERY_DIR`, when set
2. Windows: `%LOCALAPPDATA%\Cedar\recovery` (not Roaming AppData)
3. macOS: `$HOME/Library/Application Support/Cedar/recovery`
4. Other Unix: absolute `$XDG_DATA_HOME/cedar/recovery`, otherwise
   `$HOME/.local/share/cedar/recovery`

A relative `XDG_DATA_HOME` is ignored. A relative explicit override, missing
required home/app-data setting, unsafe storage, or unavailable lock is an error.
The frontend can continue editing with a visible recovery warning. It must not
claim the current text is recoverable after such an error.

On Unix, new directories are created with mode `0700`, and new lock, temporary,
and record files with `0600` (subject to a more restrictive umask). The store
rejects an existing leaf directory or file owned by another UID or with any
allowed group/other mode bits. Existing data is not silently chmodded. Symlinks
in the store path, symlink records/locks, special files, and Unix hard-linked
files are rejected. Root and lock inode/device identity are rechecked while the
store is open. A public parent such as `/tmp` is allowed; the recovery leaf must
be private. Use a private, trusted local filesystem rather than a shared/network
folder or a synchronized folder if that would share source without permission.

Windows checks reject reparse points, including directory junctions, and open
with `FILE_FLAG_OPEN_REPARSE_POINT`. Windows file/directory ACLs are inherited;
this crate does **not** inspect or repair ACLs or promise Unix-equivalent
owner-only permissions. The normal per-user Local AppData location is preferable to
an arbitrary shared override. Stable `std` metadata does not provide the Unix
inode-identity checks used here on Windows.

## Unavailable storage and quitting

Recovery storage and workspace Save are separate operations. A failed recovery
open or write must not mark the current draft backed up, reverse a successful
workspace Save, or remove its selection and Undo history. Save and Save All
retain their ordinary conditional-write and unknown-result protections.

The frontend distinguishes recovery startup, ready storage, unavailable storage,
and a stopped worker. A rejected request that never reached storage is different
from a write or removal that failed after it may already have changed storage.
Older acknowledged or uncertain copy evidence survives a rejected newer request.
“No owned copy” describes this session's evidence; it does not assert that the
recovery directory contains no older records.

The ordinary Discard and quit action still requests removal of owned copies. A terminal
recovery failure offers a decision instead of claiming deletion succeeded.
“Quit without deleting remaining recovery copies” is a separate explicit choice:
current unsaved editor text may be lost, remaining copies may be older, and a
removal already in progress may have completed. Existing records are not silently
moved, repaired, or deleted to make this path work.

Before the final confirmation, the worker freezes new recovery mutations,
cancels queued removals, and drains writes already accepted plus any operation
already running. Only an actual settlement acknowledgement can permit quitting.
The UI observes each discard or settlement wait for at most five seconds; this
does not interrupt a filesystem call. A timeout or uncertain worker state keeps
quitting blocked and offers Keep editing. A late acknowledgement cannot approve
an expired close attempt. Keep editing invalidates that attempt, and recovery
admission resumes only after the worker has settled. Retry is unavailable while
a close or resume transaction is unresolved.

The final close checks the same document versions and workspace/session state,
as well as pending saves, Save All and tool cleanup. New edits or work require a
new decision. Retaining copies never automatically schedules their removal on
Retry. These lifecycle rules do not add Windows ACL enforcement or a new privacy
guarantee; the platform limitations above still apply.

## Format and integrity

Each record is named `<64-lowercase-hex-SHA-256>.draft`. Its ID hashes the canonical
Serde JSON tuple `("cedar-recovery-key-v1", typed_identity, relative_path)`. Field
order and the type tag are fixed in format v1. Strings are not lowercased,
Unicode-normalized, or concatenated ambiguously. Callers should use the canonical
root returned by the workspace handshake. Different aliases or representations
can intentionally produce separate identities; this is not a filesystem identity
resolver.

The v1 record is:

1. Eight-byte `CEDARDR1` marker
2. Four-byte little-endian metadata length
3. A 32-byte SHA-256 digest of the complete serialized metadata header
4. Bounded UTF-8 JSON header with explicit `version: 1`, identity/path, original
   revision, timestamp, both byte lengths, and separate SHA-256 payload digests
5. Exactly `text_bytes` of raw UTF-8 draft text
6. Exactly `base_text_bytes` of raw UTF-8 saved/base text

Using raw text payloads prevents JSON escaping from multiplying normal file
sizes. Valid text bytes, including CRLF, BOM, combining characters, emoji,
embedded NULs, and trailing newlines, are preserved without truncation.
Unknown versions or header fields, malformed/truncated metadata, inconsistent
lengths, wrong filenames, nonportable paths, invalid UTF-8, unexpected trailing
bytes, or metadata/payload digest mismatches are errors. The metadata checksum
protects the original revision and timestamp even against parse-valid accidental
changes. Digests detect accidental damage; they are not authentication against a
process that can rewrite the private store.

`list()` reads only bounded headers and file metadata. It returns metadata with
no draft/base strings, plus individual issues, so one damaged record does not
hide healthy records. It deliberately does not read or checksum every payload
at startup. A payload can therefore appear in the list and fail integrity checks
when explicitly restored. `read(id)` reads and verifies only that individual
record. A write/removal also validates the existing record before replacing or
removing it so damaged evidence is not erased by routine autosave/cleanup.
Unknown entries and interrupted-write temporaries are reported and retained.
There is no automatic repair, old-draft eviction, or silent evidence cleanup.

## Bounds

Hard limits (the `Limits` API may reduce, never increase, selected quotas):

| Item | Hard bound |
| --- | --- |
| Draft UTF-8 text | 1 MiB |
| Saved/base UTF-8 text | 1 MiB |
| Relative path/root/agent-path field | 4,096 UTF-8 bytes each |
| Portable relative-path component | 255 UTF-8 bytes |
| SSH host/alias | 1,024 UTF-8 bytes |
| Optional base revision | 256 UTF-8 bytes |
| Metadata JSON header | 64 KiB |
| Serialized record | 2 MiB + 64 KiB + 44 bytes |
| Whole store including temporary replacement headroom | 256 MiB |
| Recognized record files | 128 |
| Total directory entries inspected | 256 |
| Distinct per-process sequence/tombstone keys | 4,096 |

All sizes are byte sizes. Metadata identifiers are 64 lowercase hexadecimal
characters, port is a nonzero `u16`, and timestamp/operation sequence are `u64`.
Identity/revision strings reject controls and empty values. Relative paths reject
empty components, `.`/`..`, absolute paths, backslashes, drive/alternate-stream
syntax, Windows device aliases, controls, reserved punctuation, and trailing
spaces/dots. Rejection is explicit; no normalization or truncation silently
changes the user's path or text.

The store holds 32 maximum-size normal buffers, each with its full base text,
comfortably inside its default budget. Quotas include unrecognized files,
damaged records, interrupted temporary files, and the full transient replacement
copy. An unsafe special/directory entry blocks new writes because its total
contents cannot be safely accounted for. Listing remains bounded and reports
incompleteness when the entry limit is exceeded. Exceeding a quota leaves old
records in place and returns an error; it never makes room by pruning them.
An explicit valid-record removal remains possible when byte/count quotas are
full.

## Writer and acknowledgement contract

`Store::open` acquires an exclusive, nonblocking `std::fs::File::try_lock` on
`.cedar-lock`, held for the object's lifetime. A second process gets `Error::Locked`
rather than waiting or writing concurrently. The lock file is intentionally not
unlinked when closing; removing it would permit two independent lock inodes.
Normal `Store` destruction explicitly unlocks its owned file description before
closing it. Closing the file alone is insufficient on Unix: a concurrent
fork/exec can temporarily inherit the same open-file description and retain its
`flock`, even though the descriptor is marked close-on-exec. An immediate reopen
must not spuriously report a live competing editor during that interval.

The creator process ID is retained only in memory. Only that process may use the
store or explicitly unlock it. An inherited `Store` rejects operations before
stale-sequence handling, and its destructor does not unlock the live parent's
lock. A newly opened store uses an independent description; closing an old
inherited/duplicated descriptor cannot release the new owner's lock. Failed
lock contenders never acquire an unlock-owning `Store`.

The destructor is nonpanicking and retries only an interrupted unlock syscall;
other unlock failures fall back to closing the descriptor. Broken filesystem
locking can therefore still prevent immediate release. Abrupt termination such
as SIGKILL bypasses the destructor: the OS releases the lock when its last shared
open-file description closes, so a surviving inherited descriptor can retain it
until exec/close. There is no attempt to break another process's lock or bypass
contention. A failed open remains a visible recovery warning, not a successful
persistence acknowledgement.

The frontend worker supplies globally monotonic operation sequence values. The
store retains the highest observed sequence **per identity/path**, allowing a
coalescing worker to service independent keys in a different order. A same-key
write/removal with an equal or older sequence returns `IgnoredStale`. Removals
create an in-memory tombstone even if no file exists, so a late queued write
cannot resurrect a discarded draft. Failed operations reserve their sequence as
well: retry using a newer sequence. The bounded map is reset only when a newly
locked `Store` opens; it does not change persisted base revisions.

A write checks identity, payload sizes, existing-record integrity, and capacity;
creates a private randomized temporary **in the same directory**; writes the full
record; calls `sync_all()` on the temporary; atomically replaces the destination;
and, on Unix, calls `sync_all()` on the containing directory. A removal is also
followed by directory synchronization on Unix. Accepted directory ancestors are synchronized in bottom-up order during Unix
open, including already existing paths left by an interrupted or failed prior
open. Reopening therefore retries any parent-directory barrier that may not have
completed after directory creation.

Only `MutationOutcome::Applied` acknowledges the requested write/removal.
`IgnoredStale`, enqueueing, debounce expiry, an I/O error, or a failed final sync
is **not** a durable acknowledgement for that draft version. If rename succeeded
but directory sync failed, an error is still returned; the on-disk version can
have changed, so the frontend must show uncertainty rather than success. A crash
before acknowledgement can retain either the previous complete record or the
new complete record; a crash before debounce/write can lose those latest edits.

On Windows, persistence clears the temporary-file attribute through
`NamedTempFile::keep`, immediately restores an RAII cleanup guard for the source
path, flushes the file again, and uses Rust 1.99 `std::fs::rename`. That maintained
implementation falls back from legacy `MoveFileExW` to `FileRenameInfoEx` with
POSIX replacement semantics when a compatible destination reader remains open.
A reader that denies delete sharing, an ACL denial, or an unsupported filesystem
still produces an explicit error; the old draft remains and there is no success
acknowledgement. There is no delete-then-rename gap or blanket access-error retry.
The concurrent-reader test still requires every replacement to succeed and every
read to return a complete old/new record. A held-reader regression requires the
old handle to retain its complete old record while new opens see the replacement;
a separate Windows restrictive-sharing test requires failure, preservation,
cleanup, and successful newer-sequence retry after the restrictive handle closes.
See [Rust's rename contract](https://doc.rust-lang.org/std/fs/fn.rename.html) and
[Microsoft's rename flag semantics](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information).

The Windows temporary file is flushed before atomic replacement, but `std`
has no portable containing-directory durability barrier in this implementation.
Consequently Windows does **not** have the same power-loss guarantee claimed by
the Unix fsync sequence. Even on Unix, storage hardware, mount options, broken
filesystems, or a lying flush implementation can invalidate durability promises.
The automated process-kill test verifies process-crash recovery on the tested
Linux filesystem; it does not simulate power loss or verify hardware behavior.

## Security and runtime boundaries

Private permissions and no-follow checks are defense in depth, not an OS sandbox.
A process with the user's authority can already read/modify their draft data.
The implementation uses path-based directory traversal, bounded metadata checks,
no-follow final opens, and Unix inode checks; these do not eliminate every rename
race in ancestors or between a final check and replacement/removal. A hostile
same-account writer can change the filesystem after a check. Advisory locks
coordinate cooperating Cedar processes only. A hostile/shared filesystem needs
an OS account/sandbox boundary and stronger platform-specific directory-handle
operations; this crate does not claim that protection.

Local checks include Windows MSVC-target compilation and strict Clippy. The
first published phase-3 Windows CI exposed the legacy open-reader replacement
failure described above. The 0.3.1 hotfix subsequently passed complete Ubuntu and
Windows CI at the same commit, `9f9a72608d7ea944d5946656f8fdf259a950891d`:
[verified CI run](https://github.com/LLLLimbo/cedar-ide/actions/runs/37627832872).
All 29 Windows recovery tests passed, including strict concurrent replacement,
held-reader replacement, and restrictive-sharing failure/preservation/retry;
release builds and stdio process integration passed too. Windows ACL behavior,
antivirus interference, and hardware power-loss semantics remain unverified.
macOS runtime is also unverified. A cross-compile alone is not runtime validation.

## Focused verification

Run from the repository root with the workspace Rust 1.99 toolchain:

```sh
cargo test -p cedar-recovery --locked
cargo clippy -p cedar-recovery --all-targets --locked -- -D warnings
cargo clippy -p cedar-recovery --all-targets --target x86_64-pc-windows-msvc --locked -- -D warnings
```

The suite covers exact Unicode/base-revision round trips; all SSH identity
components; no workspace modification; metadata-only startup; new-file revisions;
atomic replacement under concurrent and held readers; Windows restrictive-share
failure preservation and retry; stale writes/deletes and tombstones;
bounded tombstone memory; failed-write retry ordering; restart with old revisions;
independent subprocess lock contention; deterministic held-descriptor and
fork-before-exec release regressions; inherited-process ownership rejection;
250 immediate reopens alongside 80 parallel fork/exec launches; actual
subprocess termination after a durable acknowledgement followed by lock reacquisition and exact restoration;
corrupt/oversized/truncated records and metadata/payload checksums; parse-valid
revision/timestamp corruption; injected failed-parent-sync retry coverage; invalid UTF-8; unsupported
formats/fields; quota/headroom/count exhaustion without eviction; unknown/crash
files; strict portable traversal/device rejection; Unix permissions, symlink,
hardlink and FIFO attacks; and replacement of an open store root/lock.

All test data is synthetic. No user's workspace, SSH server, credentials, or
external service is involved.


### Lock lifecycle regression found during phase 4

A later parallel full-workspace run exposed intermittent `Locked` failures in
three tests that reopened a just-dropped store. A held `File::try_clone` reproduced
the failure deterministically without timing or retries, confirming Unix shared
open-file-description lifetime as the cause. The owner-PID-gated explicit unlock
fix changes no disk format or public API. Tests retain real contention, hold an
actual child in its pre-exec window using raw signal-safe socket I/O, verify
inherited/stale operations fail closed, and ensure an old descriptor cannot
unlock a newly acquired store. This was an availability defect; previously
acknowledged draft contents are not changed by the fix.


### Windows open-reader replacement regression

The first public phase-3 Windows CI failed an atomic replacement with OS error 5
while a concurrent reader was active. The previous `tempfile` 3.27 persistence
path used only legacy `MoveFileExW`. Microsoft's documented legacy open-target
restriction is consistent with that failure; Rust 1.99 already implements the
modern POSIX-semantics fallback. Recovery now uses the maintained standard-library
rename implementation on Windows, while preserving temporary-attribute clearing,
flush-before-acknowledgement, cleanup, and error reporting. The strict concurrent
read/write test was not relaxed or skipped. Its failure cleanup now joins the
reader before dropping the directory, avoiding a misleading secondary NotFound
panic during test teardown.
