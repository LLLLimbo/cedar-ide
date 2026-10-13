# Recent Files — 0.64 bounded functional acceptance

**0.64.0 is the latest accepted bounded functional baseline.** Exact CI and
package checks passed. The bounded Linux native run passed essential functional
and integrity checks, but quit initiation was **20.436 seconds late**, failing
the strict timing gate. This is not an all-gates pass.
Recent Files is a bounded list of paths visited during the current connection.
It helps return to an open file or reopen a closed file without searching the
workspace. The checks and their limits are recorded in the
[0.64 report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE64_RECENT_FILES.md).

## Open and choose a file

Use **Ctrl+E** on Windows or Linux, or **Cmd+E** on macOS, to open Recent Files.
macOS **Ctrl+E** keeps its existing editor behavior. **This connection only**
describes the list's lifetime. **Filter recent paths** matches case-insensitive
substrings, with at most 4,096 Unicode characters in the filter. Filtering does
not scan files, send a search request or add entries. Entries are deduplicated
by exact path; filtering does not change their spelling or identity.

Each row identifies its file with an **Open** or **Closed** badge. Clicking a row
only selects it. Press **Enter** or choose **Open** / **Open closed file** to open
the selection. **Close** or **Escape** dismisses the chooser.

Opening a live file uses its existing editor buffer, including an unsaved dirty
draft, with no Read. Opening a closed file uses the ordinary Open/Read path and
its current workspace boundaries, with the ordinary Open default caret position.
Recent Files stores and restores no closed caret, selection or draft text; it
does not promise line 1. A remembered path is not a guarantee that the file
still exists or is readable.

The list has no preview, edited-files-only switch, background index or file
discovery. Use [Find and return](FIND_AND_RETURN.md) for filename discovery,
project text search and explicit preview. Use the existing location-history
Back/Forward commands for an exact return to an unchanged open buffer.

## What enters the list

Only a settled active file with a known revision is eligible. A previously
unsaved active tab can become eligible on its first successful Save. A pending
or failed Open does not create a remembered path. Entries are paths, not copies
of file contents or editor positions. Closing an eligible tab keeps its path
in the list without keeping the closed buffer's text there.

The list holds at most **64 paths**, **256 KiB of UTF-8 path bytes in total**, and
**4,096 UTF-8 bytes per path**. These are bounded session entries, not an inventory
of the workspace or a promise to retain every previously opened file.

## Clear and connection lifetime

**Clear recent** empties the remembered list and closes the chooser.
Disconnecting, reconnecting, changing the workspace or restarting the app also
clears it. Entries do not survive a connection change, even when the next
connection uses the same workspace.

After Clear or reconnect, the still-active tab does not reappear just because
the app redraws. A new qualifying activation can add an entry. The list is not
saved to disk; ordinary editor buffers, dirty-close confirmation and explicit
saving retain their own existing behavior.

## Validation boundary

Final-version aggregate, Python and strict Clippy checks passed, including the
Recent/ordinary-Open controls.
Final formatting and current-version normal-agent proofs also passed. Exact
dual-platform CI and all three exact-source package audits passed. The bounded
Linux native functional and integrity observations do not erase its strict
timing failure, as recorded in the
[report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE64_RECENT_FILES.md). Recent Files adds no protocol or
backend command, execution permission, persistent recent-location store or
IDEA parity claim. Windows and macOS GUI behavior still need their own native
evidence.
