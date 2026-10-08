# Verification report · Literal replace preview / 0.19.0 · 2026-10-08

This checkpoint adds explicit literal replacement to the active editor buffer.
The existing Find bar gains a replacement field, Preview one / Preview all and
Apply / Cancel. No regular expressions, project-wide replacement, backend
operation or execution permission is introduced. Save remains a separate action.

The previous [0.18.0 exact Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37818623076)
and portable ZIP verification passed. Its keyboard navigation and actual Linux
trust-off evidence are retained in the [phase-18 report](TEST_REPORT_PHASE18.md).
The intermittent upstream JDT diagnostic-publication cause remains unresolved.

## Transaction contract

Matching is case-sensitive literal text. An empty query is invalid; an empty
replacement deletes the chosen match or matches. Replace one uses an exact
selected match, otherwise the next match from the cursor with wraparound.
Replace all uses non-overlapping matches from the captured source.

The preview binds the exact document/path, connection and navigation identity,
source text/edit version, query/replacement and editor selection. Apply checks
that same snapshot after the editor frame. Later typing, selection changes,
navigation or changed replacement inputs cannot apply a stale proposal.

Apply changes only the in-memory draft through the existing single editor
transaction. Undo restores the previous text and Redo restores the replacement.
Saved text, disk revision and ordinary recovery/language synchronization retain
their existing behavior. Identical output does not create a history checkpoint
or increment the edit version. No write is submitted automatically.

## Bounds and acceptance

Plans beyond 10,000 matches or the existing one-MiB document/result limit are
rejected rather than applying a truncated prefix. Preview display bounds must
be visible and separate from the complete replacement count. Unicode scalar
cursor positions and UTF-8 byte limits are handled separately; literal matching
does not normalize the document's text or line endings.

Headless acceptance covers one/all planning, empty inputs, Unicode/CRLF, exact
selection and stale-source guards, byte/count bounds, no-op history, Undo/Redo,
and actual Find/preview input frames. Native Linux verification uses only newly
generated files with execution trust off and compares disk hashes afterward.
Windows GUI, authenticated SSH and GUI Trust-on testing remain separate gaps.

## Verification status

The final local aggregate passed 808 Rust tests across 40 suites, with 22 opt-in
cases retained for their explicit process/native CI stages. All 15 new planner,
transaction and actual-frame replacement tests passed, along with the existing
navigation regressions. Independent static review found no remaining blocker.

Strict host and MSVC Clippy, formatting and diff checks passed. The normal
default-feature release app/agent build and all four actual-agent protocol,
capability, language-bridge and task-bridge smoke checks passed. Python checks
passed: export two, bundle 28 plus one
platform skip, Git fixtures six, crash collector 70 plus two platform skips,
process observer 79 and GC collector 36. Exact dual-platform CI and the
regenerated bundle remain required for this commit.

The inline preview has a 120-pixel scrolling body with Apply/Cancel outside it.
While Find is open, background drag-to-scroll is disabled to prevent egui's
previous editor rectangle from intercepting newly visible preview buttons.
Text selection, wheel scrolling and scrollbars remain available. Actual click
regressions cover the first visible Apply/Cancel at the minimum window size.
## Native Linux trust-off acceptance

The final default-feature release passed 21 recorded checks on five newly
generated synthetic files with execution trust off. Preview and Cancel left
buffers unchanged; selected replacement and all-match replacement worked, with
one Undo/Redo restoring/reapplying the complete transaction. Later typing expired
the preview and removed Apply. Literal punctuation and `$&` remained literal;
empty replacement deleted matches. Mixed CJK, emoji, accented and Greek text
remained intact; CRLF markers were observed in the changed draft preview.

The bounded 100-match preview showed three locations with reachable actions.
Editor wheel scrolling and scrollbar dragging worked while Find was open. All
temporary edits were undone, all tabs were clean and every fixture disk hash
matched its baseline. No Save was requested, so this does not establish a new
CRLF save roundtrip. After clicking a different tab, Undo requires editor focus;
Apply restores that focus for its own transaction.

Tested Linux app SHA-256:
`9aa770ba30c282e6346c64365169488f67e6328ce1674a0853ea9435f6c01137`.
Tested adjacent normal agent SHA-256:
`cea965ab405c8507b52c4163edef4e403c790e6edb2ff527c09fda34a5334323`.

Native Windows/macOS GUI and authenticated SSH remain unverified. No
resource-reduction or IntelliJ IDEA equivalence claim follows from this slice.
