# Scoped Project Text search

**0.62.2 is the accepted bounded baseline.** This document defines its directory
and filename-mask addition to Project Text and its same-row double-click guard.
The exact public source passed dual-platform CI and all three package audits;
a fresh exact-package Linux journey separately covered the scoped `src` / `*.rs`
flow and selection, explicit Preview and genuine operating-system double-click
Open in both Find views. The optional invalid-scope, empty-mask, broader OR-mask,
edited-tuple inactivity, modal-reopen and narrow-layout native checks were not
run. They must not be inferred from deterministic coverage. See the
[accepted evidence and historical failures](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT.md#accepted-0622-bounded-scoped-search-baseline).

The 0.62.0 CI failure and 0.62.1 confirmed cross-widget defect remain historical
failures. The cause of 0.62.1's earlier unexpected native Open is still unknown.
The [0.63.2 shell candidate](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE63_BROWSING_SHELL.md) reorganizes the
frontend; it has separate pending gates and does not change this search contract.
Windows GUI and real SSH remain unverified.

This is a subset of roadmap B2. It preserves the retained grouped-result,
explicit preview, ordinary Open and Back flow described in
[Find and return](FIND_AND_RETURN.md). It does not complete B2 or change the
roadmap's product decision.

## Submit a directory and filename mask

The shared **Project Text** window and bottom **FIND RESULTS** tool edit the
same query, **Directory** and **File masks** fields. Submit with **Find** in the
shared window or **Search** in the bottom tool, or Enter in a search field.
Editing, opening, hiding or switching views does not start a search. There is
at most one outstanding text-search request; editing its fields retires its
intent but does not free that physical request slot or interrupt a blocking
filesystem call.

**Directory** is an exact workspace-relative directory. Empty means the
workspace root. Use `/`, for example `src/main`; spelling, spaces and Unicode
are preserved without trimming, case folding or path normalization. A nonempty
path is limited to 4,096 UTF-8 bytes and rejects absolute paths, backslashes,
colons, control characters, empty components, `.` and `..`. A scope inside a
fixed excluded directory is rejected. The selected path must resolve to a
directory through the existing backend ancestor checks.

**File masks** is empty for all eligible filenames, or a semicolon-separated
list such as `*.rs; *.toml`. Whitespace around each separated pattern is trimmed
by the interface; whitespace inside a pattern is literal. Empty patterns,
including a whitespace-only field or a trailing semicolon, are invalid. There
may be at most **eight patterns**, totaling **256 UTF-8 bytes** after that
trimming. The wire request carries the resulting ordered list, not the raw
semicolon-separated field.

Each mask matches the **whole basename**, including its extension, with
case-sensitive literal characters and only these two wildcards:

- `*` matches zero or more Unicode scalar values.
- `?` matches exactly one Unicode scalar value.

Masks are alternatives: a file qualifies when any mask matches. An empty list
includes every eligible basename. `*.rs` matches `main.rs`, not `main.RS`;
`?.rs` matches `雪.rs`, not `main.rs`. A precomposed `é` is one scalar value,
whereas an `e` followed by a combining accent is two. No Unicode normalization
or full case folding is performed. Dotfiles are not implicitly hidden.

There is no regex, gitignore, character-class, brace-expansion or path-pattern
mode. Patterns containing `/`, `\`, `;`, `[`, `]`, `{`, `}`, `!` or control
characters are rejected. Other punctuation is literal, for example
`file.(rs)` matches that exact filename. Repeated stars have ordinary `*`
semantics; `**` does not create recursive path matching. Use Directory to
choose a subtree, not a path embedded in a mask.

The text query remains a **case-sensitive literal substring** of disk text.
For the scoped operation it is nonempty, at most 4,096 UTF-8 bytes, and contains
no CR, LF or NUL. The interface does not submit a whitespace-only query.
Query whitespace is otherwise preserved. Each matching line contributes one
historical line result, even if the query occurs several times on that line.
Unsaved draft bytes do not participate in the search.

## Compatibility and wire contract

Protocol **4**, AgentInfo schema **1**, existing capability meanings, the
32-direct-capability and two-group limits remain unchanged. The addition is a
separate optional top-level Hello descriptor, `scoped_search`, with exactly one
required positive `u32` member, `version`. Only **version 1** enables the new
operation. The version identifies this extension's contract, independently of
the agent's product version and filename discovery's descriptor.

The descriptor is a strict object: missing, duplicate, unknown or wrong-type
members, positional arrays, zero and out-of-range versions are malformed. The
outer `scoped_search` field may occur only once, including null and escaped-name
spellings. Its compact serialized object is 13 bytes for v1 and at most 22
bytes for a positive `u32`; this is not a 22-byte raw-frame limit. Raw whitespace
and escaped JSON spellings remain subject to the existing 8 MiB frame bound.

| Submitted specification / peer | Behavior |
|---|---|
| Empty Directory and empty masks | The unchanged legacy `Search` operation and its existing capability gate, including on a v1 peer. |
| Nonempty Directory or at least one mask; supported v1 descriptor | Dedicated `SearchIn` operation, serialized as `search_in`, carrying `request`. |
| Filtered specification; descriptor absent, null or a well-formed future positive version | Explain that scoped search is unavailable; send **zero scoped-search requests** and no broad fallback search. |
| Malformed descriptor | Reject the handshake; do not probe for support. |
| Older client with a new agent | The optional addition must remain ignorable by the accepted 0.61 reader; existing operations keep their original meaning. |

A flat capability claim, product version string, retry or trial request cannot
enable scoped search. Support comes only from the first validated Hello for the
connection and is refreshed on reconnect. To use legacy search on an older
peer, the user must explicitly clear both filters and submit again; a failed or
unsupported filtered search never silently widens to the workspace.

`ScopedSearchRequest` contains exactly `query`, `scope`, `masks` and `limit`.
All are required; duplicate, unknown and wrong-type members are rejected.
`scope` and `masks` follow the rules above. A positive `limit` is capped at
**1,000 backend rows**; the interface requests **500**. The response echoes the
exact submitted request, including its original requested limit rather than
the effective capped value.

`ScopedMatches`, serialized as `scoped_matches`, contains a strict `result`
with `request`, `matches`, `incomplete`, `reasons`, `examined_entries` and
`read_bytes`. Rows retain exact relative paths, one-based line numbers and
bounded historical snippets. They are ordered by exact path and then line,
without duplicate path/line pairs. An incomplete traversal does not promise a
globally best or stable subset across runs.

The client validates the echoed tuple, scope containment, masks, portable
paths, excluded ancestors, row ordering, coordinates, snippet bounds, counters,
incomplete reasons and encoded size before exposing any rows. A mismatched,
malformed or oversized result, or the wrong payload type, is a protocol error;
the process connection is failed rather than accepting partial data or retrying
as legacy Search. Ordinary matching application errors retain their existing
transport behavior.

## Backend work and limits

The agent starts traversal at the chosen directory, including directories never
loaded in Explorer. It applies basename masks **before opening file contents**.
This is backend scope, not filtering an earlier workspace-wide result. Matching
and nonmatching entries both consume the traversal entry budget; narrower masks
reduce eligible content reads but do not necessarily reduce directory traversal.

Each scoped search has independent bounds:

- 20,000 examined directory entries and depth 64 below the selected scope
- One cooperative five-second elapsed budget for traversal, reads and matching
- 4,096 pending directories and 512 KiB of pending relative-path text
- 4,096 UTF-8 bytes for each relative path
- 1 MiB of text per eligible file
- 32 MiB of **actual bytes read**, including bytes later rejected as binary or
  invalid UTF-8, partial reads before an error, and a growth-detection byte
- 1,000 maximum backend rows, with 500 requested and retained by the interface
- 512 KiB of compact encoded `ScopedSearchResult` JSON, including the echoed
  request, counters, reasons, JSON escaping and rows, excluding the enclosing
  response framing

The retained interface also keeps its existing 512 KiB combined path/snippet
text bound. Snippets remain bounded historical line previews; a long query can
extend beyond the cropped snippet. None of these limits turns the cooperative
deadline into a hard wall-clock guarantee: an individual blocking filesystem
call can outlast it. Limits are not renewed as matches arrive.

Fixed excluded directory names are `.git`, `target`, `build`, `dist`,
`node_modules`, `.idea`, `.venv`, `venv` and `__pycache__`, matched exactly.
These comparisons use component-name spelling. Filesystem aliases, including
case variants on case-insensitive filesystems, are not checked for equivalence.
Links, Windows reparse points and special files are excluded. Oversized files,
NUL-containing binary text and invalid UTF-8 are excluded from content results.
These declared exclusions do not alone make a scan incomplete. There is no
`.gitignore` or IDE module-exclusion interpretation. Ancestor and leaf checks
reuse the existing path boundary; they are not a sandbox against hostile
concurrent filesystem replacement.

Incomplete responses retain distinct, deduplicated reasons:
`permission_denied`, `io_error`, `depth_limit`, `time_limit`, `entry_limit`,
`stack_limit`, `response_limit`, `result_limit`, `unrepresentable_path`,
`unsafe_path` and `read_byte_limit`. `incomplete` is true exactly when the reason
list is nonempty. Counters and reasons describe the submitted scan, not newer
field edits. Permission and I/O problems are not recast as an empty successful
search. Paths that cannot be represented safely are not rewritten lossily.

**No matches in an incomplete scan does not establish absence.** A complete
no-match result applies only to the submitted directory, masks and exclusion
policy. A narrower Directory can address traversal limits; changing query text
or masks alone may not. Search never enables execution trust, runs Git, starts
a language server or creates an index or filesystem watcher.

## Retained identity, preview and Open

Long submitted summaries and path-group labels occupy one line; hover reveals
the full text, including on inactive path groups. Truncation affects presentation
only, never the request, result path or action identity.

The accepted snapshot retains the complete submitted query/directory/mask-list/
limit tuple, extension version or legacy mode, workspace, connection generation
and request identity. Both views show that same historical snapshot and its
submitted filters. Current field contents cannot relabel old results as a new
search.

Editing **any** query or filter field immediately deactivates historical rows
and retires pending preview/Open intent. Editing back to the previous value,
including an equivalent mask spelling after separator trimming, does not
reactivate them. Starting a rerun, a failed rerun and reconnect also leave old
rows inactive until an explicit successful submission for the current tuple and
connection. Late success, error, preview replies or captured input cannot
activate a different tuple.

This intentionally tightens the 0.61 interface behavior for an in-flight legacy
Search too: editing its query now discards the late completion instead of
replacing the displayed historical snapshot with another inactive result set.
The prior snapshot remains visible and inactive until a fresh explicit
submission succeeds. Editing back to the original text does not revive the old
request. Legacy wire messages and search-engine behavior are unchanged.

The existing exact-path grouping, group collapse state, selected hit, bounded
scroll retention, hide/reopen and Open–Back flow is preserved. Click selects;
Space explicitly previews; Enter or double-click performs ordinary Open.
Hidden or fully clipped hits cannot activate. A successful rerun replaces the one
accepted snapshot; there is no cross-restart search history.

The 0.62.2 guard requires a preceding accepted clean click on the **same
exact result in the same view and preview pane** before either surface can use
the interface framework's double-click classification for Open. A rapid group
or other-row click followed by the first click on a result only selects that
result. Intervening controls, rejected or mixed input, focus moving elsewhere or
leaving the window, field edits, result replacement, navigation and hide/reopen
break the sequence. Clicks do not carry double-click authority between the
shared window and bottom tool.
Genuine same-row doubles retain the framework's timing; repeated layout passes
cannot count one release twice. A triple-click classification does not itself
perform Open. The two surfaces keep their existing focus, keyboard and Escape
behavior. The accepted evidence remains bounded to the checks listed above.

Preview of an already open file uses its **current buffer**, including a dirty
draft, with zero backend Reads. Ordinary Open reuses that dirty tab and keeps
its text and Undo history. A text hit navigates to its **historical position**
in the current buffer; it does not prove the original match is still there.
Unopened files use the existing bounded Read path. Search results carry no
revision that guarantees freshness. Preview, Open and Back do not implicitly
save, replace a dirty draft or promote preview bytes into an editable document.

## Verification boundary and non-goals

The accepted 0.62.2 evidence covers protocol compatibility, backend
traversal/read bounds, client gates and both frontend surfaces. The deterministic
and normal-agent checks include the actual accepted 0.61 reader, unsupported
peers receiving zero new requests, out-of-scope and nonmatching files receiving
zero content opens, independent incomplete reasons, malformed result rejection
and the retained dirty-buffer journey. These checks establish their own bounded
contracts; they do not supply unobserved native input, geometry or request counts.
Later shell changes require new evidence for the changed presentation and focus
paths. Source inspection alone does not establish that acceptance.

This addition does not claim case-insensitive text search, whole-word search,
regex, live or debounced queries, CamelCase matching, persistent indexing,
broader semantic navigation, new Windows GUI or real-SSH acceptance, or
IntelliJ IDEA parity. The older 0.61 evidence remains specific to its retained
results behavior; the filtered-search evidence belongs to 0.62.2.
