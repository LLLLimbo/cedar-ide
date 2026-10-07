# Ordinary workspace saves

Workspace saves replace an existing regular text file only when its SHA-256
matches the revision supplied by the caller. The revision is checked before
preparing a temporary file and again immediately before committing it. A missing
file with an expected revision is a conflict, not permission to recreate it.
Creating a new file without a revision continues to use
`NamedTempFile::persist_noclobber`: a racing creator wins rather than being
overwritten.

## Replacement sequence

1. Validate the relative path, workspace boundary, regular-file type, expected
   revision, and read-only permission. Symlinks are rejected.
2. Write a same-directory `.cedar-save-` temporary file, flush the Rust writer,
   and copy the existing file's portable permissions.
3. On Windows, use `NamedTempFile::keep` to clear `FILE_ATTRIBUTE_TEMPORARY`, then
   immediately restore temporary-path cleanup ownership. Flush the prepared
   file with `sync_all` on every platform.
4. Repeat path validation and read/hash the destination. If it was edited or
   removed during preparation, return a conflict and clean up the temporary.
5. Commit immediately after the final revision check. There is no further
   attribute change or flush between that check and the rename. Non-Windows
   replacement retains tempfile persistence; Windows uses `std::fs::rename`.
6. Return the SHA-256 of the saved bytes. Unix additionally attempts to sync
   the containing directory; a failure after commit does not turn a completed
   save into an ambiguous error response.

The Windows change avoids tempfile 3.27's legacy-only replacement operation.
[Rust's rename implementation](https://doc.rust-lang.org/std/fs/fn.rename.html)
has a maintained `SetFileInformationByHandle` fallback. On supported Windows
filesystems, its POSIX replacement semantics let an existing delete-sharing
reader finish reading the old complete file while later opens see the new
complete file. Microsoft's
[rename flags documentation](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information)
describes this old-handle/new-open behavior. Read-only and deny-delete-sharing
restrictions remain errors: the
[Rust 1.99 source](https://github.com/rust-lang/rust/blob/1.99.0/library/std/src/sys/fs/windows.rs)
sets replacement and POSIX flags without the separate ignore-read-only flag.
The implementation never deletes the destination first or adds an
application-level automatic retry.

## Limits

These are optimistic conflict checks, **not atomic compare-and-swap** against
external writers. A different process can still change a destination or an
ancestor between validation and rename. Workspace confinement assumes a trusted
filesystem; it is not an OS security sandbox. A successful response describes
the completed save, not protection against later external edits.

Flushing the prepared file does not establish a portable Windows directory
durability barrier. Unix directory syncing is best effort here. These ordinary
saves do not promise survival of every power-loss, device, filesystem, ACL,
network-share, antivirus, or hostile same-account interference scenario.

## Regression coverage and verification

The workspace tests cover:

- A held reader retaining exact old bytes while fresh opens and the returned
  SHA-256 identify the complete new contents
- 64 consecutive successful replacements under a concurrent reader, with no
  partial/interleaved contents or missing path and no leftover temporary files
- Windows deny-delete-sharing failures preserving the original contents and
  revision, removing the temporary, and succeeding on an explicit retry after
  the reader closes
- Windows new and replaced files without `FILE_ATTRIBUTE_TEMPORARY`
- Windows read-only rejection, including an attribute added after preparation
- Existing Unix mode/hardlink preservation and read-only/symlink rejection
- Exactly one racing new-file creator whose acknowledged contents and hash
  match disk, with no temporary-file residue
- Per-call deterministic preparation hooks proving early stale-revision
  rejection and late external-edit, removal, creation, and Unix symlink checks

Run from the repository root with Rust 1.99:

```sh
cargo fmt -p cedar-workspace -- --check
cargo test -p cedar-workspace --all-targets --all-features --locked
cargo clippy -p cedar-workspace --all-targets --all-features --locked -- -D warnings
cargo clippy -p cedar-workspace --all-targets --all-features --target x86_64-pc-windows-msvc --locked -- -D warnings
```

Focused Linux tests and Linux/Windows-target strict Clippy passed during this
implementation. Windows-target compilation is not Windows runtime validation;
the actual Windows CI run must pass before this change is considered verified
on Windows. All fixtures use synthetic files in temporary directories.
