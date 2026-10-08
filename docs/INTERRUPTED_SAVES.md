# Check an interrupted save

A connection can end after the agent writes a file but before Cedar receives the
save acknowledgement. The tab now keeps a bounded, session-local SHA-256 token
for the submitted contents and shows **SAVE OUTCOME UNKNOWN**. Cedar does not
retry that write automatically. Other accepted but unanswered save requests are
conservatively retained too; this does not establish that they were transmitted.
A command that could not be queued remains an ordinary unsaved draft.

After reconnecting to the same workspace, select the affected tab and choose
**Check interrupted save**. A successful check requires two agreeing bounded Reads and sends no Write.
A failed check may stop after the first Read. It verifies the exact workspace, path, tab instance, request
lineage, returned content/revision relationship and agreement between both reads.

- If disk matches the submitted contents, Cedar adopts that read-back text and
  revision as the saved baseline. The current editor text, cursor, edit version
  and Undo/Redo history stay unchanged. A newer draft remains unsaved against
  the newly confirmed baseline.
- If disk matches the original baseline, the existing baseline stays in place.
  Save is available again for a separate, explicit user action.
- For an originally new file, two not-found reads can confirm current absence.
  The draft remains new and unsaved; only a later explicit Save creates it.
- A missing previously existing file, different contents, an unreadable file or
  an invalid response leaves the interrupted state unresolved. Use Compare with
  disk and Copy draft to review. Cedar does not silently adopt a divergent base,
  recreate an existing file or overwrite someone else's changes.

Typing, Undo, changing tabs, reconnecting, closing/reopening a tab or changing a
related task-profile form during a check invalidates its result. Run the check
again. Duplicate clicks and overlapping saves/checks do not enqueue unbounded
work. Adoption occurs only after the native frame's editor/form input handlers.
An unresolved save stays protected even if Undo returns to the old baseline.

## What the result proves

A successful check establishes current content equivalence. It does not prove
which request wrote those bytes, that the original request committed, or that
the same physical file still exists. Deleting and recreating a file with identical
bytes is indistinguishable under the current content-based protocol. A new file
may also have been created and deleted before two reads found it absent.

The token uses a collision-resistant SHA-256 fingerprint without retaining an
additional full submitted draft. Read-back revisions must match SHA-256 of their
actual contents, and both read-back snapshots must agree. Files are bounded to
1 MiB, paths to 4 KiB, with one token per tab and one active check.

These are two observations, not a filesystem transaction or ongoing watch.
Later saves still use Cedar's existing revision check, which is not an atomic
compare-and-swap. Ordinary Written acknowledgements retain their existing
revision handling; this feature's stronger validation applies to reconciliation
Reads and does not redefine the wire protocol.

## Recovery and scope

The interrupted-save token lasts only in the current app session. Existing draft
recovery remains separate: an owned newer draft receives the updated baseline;
only an owned clean recovery copy may be removed. Storage is confirmed only after
its own acknowledgement. An unrelated older recovery copy is preserved. Task
profile form edits and reconnect-review requirements are not cleared by checking.

Application-crash persistence of transaction tokens, three-way merge, automatic
replay and authenticated SSH interoperability are outside this slice. Local stdio
fault tests exercise a real Workspace commit followed by a deliberately missing
reply; they do not substitute for an authenticated SSH test or native GUI use.
