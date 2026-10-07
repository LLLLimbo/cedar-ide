# Private draft recovery (phase 3)

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
OS process termination releases the advisory lock. Filesystems must implement
file locking correctly; no attempt is made to break another process's lock.

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

On Windows, the temporary file is flushed before atomic replacement, but `std`
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

Windows MSVC-target compilation and strict Clippy are checked. Windows runtime,
ACL behavior, actual cross-process locking/rename under antivirus interference,
and crash/power-loss semantics have not been exercised here. macOS runtime is
also unverified. Do not describe those platforms as runtime-validated on the
basis of the cross-compile.

## Focused verification

Run from the repository root with the workspace Rust 1.99 toolchain:

```sh
cargo test -p cedar-recovery --locked
cargo clippy -p cedar-recovery --all-targets --locked -- -D warnings
cargo clippy -p cedar-recovery --all-targets --target x86_64-pc-windows-msvc --locked -- -D warnings
```

The suite covers exact Unicode/base-revision round trips; all SSH identity
components; no workspace modification; metadata-only startup; new-file revisions;
atomic replacement under concurrent readers; stale writes/deletes and tombstones;
bounded tombstone memory; failed-write retry ordering; restart with old revisions;
independent subprocess lock contention; actual subprocess termination after a
durable acknowledgement followed by lock reacquisition and exact restoration;
corrupt/oversized/truncated records and metadata/payload checksums; parse-valid
revision/timestamp corruption; injected failed-parent-sync retry coverage; invalid UTF-8; unsupported
formats/fields; quota/headroom/count exhaustion without eviction; unknown/crash
files; strict portable traversal/device rejection; Unix permissions, symlink,
hardlink and FIFO attacks; and replacement of an open store root/lock.

All test data is synthetic. No user's workspace, SSH server, credentials, or
external service is involved.
