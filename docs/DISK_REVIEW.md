# Disk comparison and clean-file reload

The editor's **Compare with disk** action reads the selected path explicitly. It
uses the existing Read operation, so local and SSH backends share the same path
without execution trust, commands, automatic polling or a file watcher.

The two read-only panes show the current draft and the disk snapshot from the
last accepted read. Copy, Refresh and Close do not modify the document or save a
file. A never-saved file can also be compared when another process created the
same path before its first save. Comparing never adopts the disk revision or
marks a dirty draft clean.

## Reload scope

**Reload clean tab is available only for an unchanged clean document.** It is
not a discard-draft or conflict-resolution shortcut. Dirty drafts retain their
text, baseline, revision, undo history and recovery ownership. Resolve their
content deliberately in the editor; this slice adds no merge engine or force
save.

Reload performs a second Read and compares both text and revision with the
version already reviewed. If disk changed again, the review is updated and a
new click is required. Missing, unreadable, unsupported or oversized files never
become permission to create or overwrite a file. The second read narrows the
race; disk can still change afterward, and later saves retain their ordinary
revision check.

An accepted clean reload is one native editor undo transaction. The accepted disk
text and revision become the saved baseline. Undo restores the former text as a
dirty draft against that new baseline; Redo returns to the accepted clean text.
The existing bounded history is retained, subject to its normal capacity. Explicit
selection changes coalesce same-text native checkpoints so they neither erase
Redo nor consume text-history slots. This can make temporary copies of the bounded
history during that interaction; it is not a new memory benchmark. An
identical snapshot is a no-op. A revision-only adoption does not manufacture a
text edit or language-service update. Cursor positions are clamped safely and
stale find/navigation positions are cleared.

Undo is an in-memory facility, not a promise of durable backup of old disk
contents. Dirty reload is deliberately deferred: making a dirty document clean
would currently remove its owned recovery record. Retaining that older record
would also occupy the one recovery slot for that workspace/path and block newer
backups until explicit review. This slice does not silently choose that tradeoff
or introduce a second recovery store.

## Freshness and frame ordering

The review is bound to connection generation, review/request ticket, document
identity/path, edit version, saved revision and navigation context. A stale
success or error cannot reopen a dismissed window, replace a newer review or
affect a reopened same-path tab. Changed drafts, saves, navigation, closing and
reconnection invalidate reload eligibility. The frontend validates returned
path, text and revision bounds as well as relying on backend file checks.

Network replies arrive before normal editor input. A verified reload is staged,
then rechecked at the final input barrier after editor, form and language-popup
actions and before queued profile Save/Run. Typing or another mutation in that
same frame therefore cannot be overwritten by an earlier reply.

Reload does not acknowledge a save or run a task-profile action. A reloaded raw
cedar.tasks.json invalidates an older profile-form source while retaining the
form's edits; existing guards block stale Save/Run. Text changes use the normal
edit-version and language synchronization path, respecting manual-sync settings
and never starting a language server. Comparison and clean reload do not claim
ownership of unrelated older recovery copies.

Only one review and one disk read are retained at a time. Repeated submission is
blocked while pending. Cancellation and close release preview state; an outstanding read still drains
before another can be submitted. Reconnection clears the prior generation;
the read-only preview reuses cached layouts rather than cloning its full text
on every frame. The editor keeps its existing bounded native undo store. Oversized editable
drafts remain intact rather than being truncated to fit a preview.

See the current [verification report](TEST_REPORT.md) for executed tests and
native interaction evidence. Protocol-path tests do not establish authenticated
SSH interoperability or Windows/macOS GUI acceptance.
