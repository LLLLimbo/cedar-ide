# Phase 77: visible Project context candidate

Version 0.77.0 is a candidate, not a published release. This document is the pre-validation scope snapshot. Final results must be reported separately against the exact sealed source.

## Scope

The Project toolbar now has a read-only, single-line mode and directory-scope label. Its source is the existing explorer mode and explorer_scope(), independently of the active editor path. Root is displayed as `/`. Non-ready connections show their actual state instead of presenting a loaded root. Native truncation preserves the full text; hover includes the original workspace root and directory scope. The label is nonselectable and hover-only, with native Label text semantics and no new focus stop or navigation action.

Default Flat, explicit Tree loading, the existing menu, four toolbar targets, scroll owners, operation and path identities, permissions, keyboard/focus handling and dirty-document behavior are unchanged. No filesystem read, background traversal, new path state, protocol operation or dependency was added. Vendor IME receipts and their notice/policy remain unchanged.

## Intended focused checks

Production raw-input/update frames with base_fonts: first visible and settled geometry at 780×540 and 1178×814, sidebar widths 180/246/460 and actual context font sizes 13/18; root/nested/long CJK scopes; selected Tree directory/file-parent/collapse; unconnected/loading/failed states; scope versus active same-name file; native full-text Label/tooltip; inert focus/Tab behavior; before/after request ledger and dirty selection/Undo/Redo. Retained sidebar, Project/Tree/reveal and normal-input tests must still pass without relaxed oracles.

## Not run at this source snapshot

All newly added tests and retained focused checks are NOT RUN here. Full Rust workspace aggregate, full Python aggregate, full workspace Clippy, both release builds, five Linux and four Windows canonical proofs, exact-public-source CI, official package audits, native GUI and real OS screen-reader/IME acceptance are NOT RUN locally for this candidate. Earlier versions are lineage evidence only. The unchanged official CI gates remain required for later acceptance. No GUI baseline rerun or public write is included in this stage.
