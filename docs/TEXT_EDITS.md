# Plain text edit planning

`crates/app/src/text_edits.rs` is a pure, reusable planner for edits against one
captured document. It does not change editor state, write files, invoke commands,
or interpret workspace/resource operations. Live rename remains outside its scope.

## API and input contract

- `parse_text_edits(&Value)` accepts `TextEdit[]` or `null`; `null` becomes an empty list
- `TextEdit` contains `range: completion::Range` and `new_text: String`
- `plan_text_edits(source, edits, cursor_chars)` returns `PlannedTextEdit`
- The plan contains the finished `text`, a Unicode-scalar `cursor_chars`, and the
  accepted proposal count `edit_count`

The plain wire shape follows the official [LSP 3.17 TextEdit definition](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/types/textEdit.md).
The [document-formatting response](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/language/formatting.md)
is a plain edit array or `null`. This client deliberately uses a stricter subset:
all edit, range, and position objects have exact fields. It rejects annotated
edits, commands, insert/replace completion edits, resource operations, unknown
fields, NUL insertions, and non-integer or out-of-domain coordinates.

## Validation and resource limits

All ranges refer to the original source, regardless of array order. Positions use
UTF-16 units and must lie on actual document boundaries. Reversed ranges,
nonexistent lines, past-line-end characters, surrogate-pair splits, and CRLF
interiors are rejected. LF, CRLF, and bare CR are recognized and preserved.

Overlapping ranges are rejected. Adjacent nonempty ranges are accepted. Any
insertion sharing another edit's start/end boundary is rejected, including two
insertions at the same point and empty insertion proposals.

Inclusive limits:

- 1,024 edits
- 1 MiB source, aggregate inserted UTF-8 bytes, and final UTF-8 bytes, independently
- Line/character values in `0..=2_147_483_647`

The parser checks the whole payload before cloning inserted text. The planner
validates every edit, range, overlap, cursor, and final size before allocating the
replacement string. Aggregate size arithmetic is checked, and bounded vector and
string reservations are fallible. Sorting costs `O(edits × log(edits))`; ordered
endpoint resolution scans the source once rather than rebuilding a line index per
edit. Auxiliary planning storage is proportional to the bounded edit count.

## Captured cursor behavior

The editor's cursor is a Unicode-scalar offset, not a byte or UTF-16 offset. An
out-of-document captured cursor or a cursor inside CRLF is rejected.

- Unchanged regions retain their relative position after preceding edit deltas
- Insertion exactly at the cursor places it after the inserted text
- A cursor inside a replacement retains its relative scalar offset, clamped to
  the replacement's scalar length
- The original replacement end maps to the new end
- A result cursor inside a newly formed CRLF moves after its LF
- If the entire result equals the source, the original cursor is retained exactly

This policy is deterministic, including adjacent nonempty edits. It avoids forcing
every cursor to EOF when a server returns one whole-document replacement.

## Caller responsibilities

Retain the original document/session/version snapshot. Preview without mutating
the editor. Revalidate that snapshot immediately before Apply, then commit the
finished replacement through one native editor transaction. Replanning with a
newly moved cursor is safe only while the original source and retained edits are
unchanged.

Use `plan.text == source` to detect every no-op, including individually changed
edits whose combined result is unchanged. Skip commit/undo/version/dirty-state
changes for a no-op. `edit_count` counts proposals, not semantic changes.

The planner does not establish target-document freshness, authorize recovery
ownership, provide cross-document atomic undo, or make workspace rename safe.

## Focused verification

Run `cargo test -p cedar-app text_edits --locked`. Inline tests cover exact schemas,
negative/floating/oversized coordinates, count and byte boundaries, Unicode and
mixed terminators, EOF, ambiguous insertions and valid adjacency, source-relative
ranges, no-op behavior, cursor transformations, reference-converter agreement,
small generated Unicode edits, and maximum-size ordered scanning.
