# Explicit draft-only merge

Compare with disk can preview a conservative merge when both a saved editor draft
and the disk file have changed since the known saved baseline. Choose Preview merge,
review the candidate, then explicitly Apply. Applying reads disk again and requires
the same bytes and revision, plus the same editor/session state at the final input
barrier. It never writes the file or starts a task/language service.

## Supported textual rule

Each side is treated as one changed region between its longest matching line prefix
and suffix. Line tokens retain their exact endings. Regions must have an unchanged
base line between them. Repeated-line insertion/deletion alignment ambiguity,
touching/overlapping regions, and interleaved changes are refused. Identical changes
can also be refused. This conservative rule does not infer programming intent or
provide a general three-way merge. Refusal leaves the current draft unchanged.

Every base, draft, disk snapshot and result is limited to 1 MiB and 65,536 line
tokens. NUL-containing text is unsupported. There is no recursive scan, automatic
refresh, external merge tool, new dependency, or background computation.

## Baseline, Undo and Save

Apply changes only the editor text. The freshly reviewed disk text and revision
become its separate saved baseline; the merged text remains dirty. One Undo restores
the previous draft and full selection. Redo restores the merged draft. The initial
Apply uses the existing collapsed/clamped primary caret behavior. Undo and Redo
retain the newly observed disk baseline, because neither operation writes disk.

Save remains a separate explicit action and uses that disk revision. If disk changes
after Apply, the existing save conflict check can reject the write. The double read
is not an atomic filesystem transaction, and matching bytes cannot establish that
the same physical file survived or that an earlier write committed.

A new file without a saved revision, a pending or unknown save, a changed draft,
selection, tab, connection, baseline or profile form, and a pending close/transaction
can make the preview ineligible. Refresh and preview again rather than applying an
old candidate. Ordinary clean reload and interrupted-save reconciliation remain
separate actions.

Recovery stores the dirty merged text with the new disk baseline only under its
existing ownership and acknowledgement rules. An older unowned recovery copy is
not discarded. When merging cedar.tasks.json, the profile form draft is retained;
queued actions and its old source are invalidated, requiring an explicit Load before
profile Save/Run. No execution trust is granted by a merge.
