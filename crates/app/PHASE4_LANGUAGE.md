# Phase 4: safe formatting and language navigation

## User flow

Start an installed stdio language server from the Language panel. Each explicit
feature action is enabled only when the server advertises its corresponding
provider and the active file matches the selected language profile.

- **Format preview** synchronizes the exact current draft, requests document
  formatting with the acknowledged LSP version, and opens a read-only Before /
  After window. Indentation is explicit (1–16 columns; spaces or tabs). Apply
  replaces only the in-memory draft in one undo transaction. Cancel, Escape,
  and closing the preview discard the proposal. Save remains separate.
- **Find references** has an explicit Include declaration option. All matching
  open drafts must be acknowledged by the server before dispatch. The returned
  list is clearly labeled an **unversioned server snapshot**. Unopened target
  files have no freshness guarantee, and any target can change after the request.
- **Refresh outline** requests a new outline once. Hierarchical document symbols
  keep their nesting and navigate via `selectionRange`; flat symbol-information
  responses stay flat and retain their container as display context. Unknown
  numeric symbol kinds use a generic label. Editing invalidates the outline.

## Snapshot and mutation boundaries

Formatting captures connection generation, language session, navigation-sensitive
request sequence, unique document ID, workspace-relative path, edit version,
source text, cursor, and acknowledged LSP version. Response handling and Apply
both check the snapshot. Tab navigation invalidates proposals immediately, even
when the user switches back before another frame. Close/reopen, reconnect,
restart, edits, cancellation, and newer feature requests also invalidate them.
Cursor-only movement is allowed for formatting and outline; Apply maps the
latest cursor through the already validated edit set. Editor Undo/Redo skips
native same-text cursor checkpoints, so navigation does not require an extra
key press before the next text change. History remains bounded and is copied
only for an explicit edit or history action. History-only key batches retain
their event order; mixed text/paste/navigation input is passed unchanged to
the native editor so queued edits cannot be reordered or dropped. Local outline selection
also invalidates older URI/file navigation so a late response cannot steal focus,
without expiring document-only formatting work for that cursor movement.

Reference requests additionally capture every participating document identity,
path, text, edit version, and acknowledged LSP version. A changed matching-open
set, changed participant, or changed source query position invalidates a pending
result. Retained reference lists remain explicitly unversioned rather than
claiming current-target freshness.

Formatting does not issue disk writes, change the saved baseline/revision, or
call recovery authorization. Ordinary recovery observation handles the new draft
and preserves any older, unowned recovery copy. Save acknowledgements remain
independent: they update the saved snapshot while preserving newer editor text.

All returned URIs remain inert. Reference and flat-outline navigation goes
through the existing `LanguageResolveUri` agent confinement and navigation
sequence checks. Open dirty target buffers are reused unchanged; every range is
validated against that target's current text, including strict UTF-16 surrogate
and CRLF boundaries. Hierarchical outline navigation uses its captured and
validated current-document selection range.

## Bounds and strict shapes

- Formatting uses the shared plain-text-edit planner: `TextEdit[] | null` only,
  up to 1,024 edits and 1 MiB source/result/aggregate replacement text. Invalid,
  overlapping, unsupported, or ambiguous changes fail the whole proposal.
  Null, empty, and identical results are no-ops without undo history.
- References accept `Location[] | null` only, not a single Location or
  LocationLink: up to 1,024 locations, 16 KiB per URI, 512 KiB retained URI text.
- Outline accepts a homogeneous hierarchical `DocumentSymbol[]` or flat
  `SymbolInformation[]`, or null. Mixed/hybrid shapes fail. Limits are 2,000
  total nodes, depth 32 (root depth 0), and 512 KiB total retained text including
  names, details/container names, and flat URIs. Hierarchical source ranges,
  selection containment, and child/parent containment are checked. Batched
  endpoint validation avoids repeated full-source scans.

None of these features execute server commands, apply workspace edits, perform
live rename, rename files, or apply server-originated edits. Cross-file rename
remains deferred because unversioned targets and omitted resource renames can
produce unsafe or incomplete changes.

## Automated evidence

`language_features_tests.rs` covers atomic formatting undo/redo, baseline and
filesystem nonmutation, stale responses and previews, double Apply, all no-op
forms, cursor-only movement, exact sync versions, reference participant changes,
save acknowledgements during editing, dirty reference targets, UTF-16 rejection,
older recovery preservation, provider/size guards, and headless layouts at
780×540 and 1320×880. Parser tests include malformed, mixed, oversized, deep,
wide, Unicode, and CRLF responses. Native-window and real-JDT checks are recorded
separately by the integration owner; headless layout tests do not substitute for
those checks.
