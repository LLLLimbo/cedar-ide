# Find Java type

Use **Find Java type** in the common header, beside Files and Project Text, to open
a focused type-search view in the Language panel and focus its query. Server
configuration, unrelated language actions and other result tabs stay in the full
Language controls. Choose **Language tools** or press Escape to return to those
controls. Session-busy, paused-sync and cleanup warnings remain visible. Active
server startup retains its existing lifecycle controls. The entry stays visible
when unavailable; hover it for the connection, trust, session or provider reason.
The full Language panel retains the same entry and availability checks.

Type search explicitly queries the index of an already running, trusted
Java language session. Enter a type name or qualified-name pattern and press Search
(or Enter in the query field). The agent must advertise `language_workspace_symbols`
and the server must advertise `workspaceSymbolProvider`.

This uses standard LSP `workspace/symbol`. Pinned JDT LS 1.61.0 searches indexed
source types, including nested types, with its own pattern/camel-case matching.
Cedar does not start a server, save files, scan directories or issue repeated searches
when the query changes. Search results are an **unversioned index snapshot** and
can lag unsaved editor changes. An empty result does not prove that no such type exists.
Opening or reopening the view sends no search or startup request, retains the query
text and preserves editor drafts and Undo. Language tools or Escape uses the
existing Language-panel dismissal behavior, retiring the result snapshot without
stopping the server or sending a new request. Java types still use their existing result view;
they are not combined with Files or Project Text rows in the shared Find window.

In the focused view, automatic tool-panel height measures the query controls,
visible warnings and result content, aiming to show up to three complete
title/container/location entries when the available window space permits. It
does not promise three visible entries in every small window, font scale or
warning combination. Dragging the tool-panel divider sets an explicit height
that remains across query replies; **Automatic height** clears that preference
and restores measured sizing. These layout actions send no search or file read.

Queries are nonempty, at most 256 UTF-8 bytes and contain no control characters.
Responses are bounded to 256 complete locations with per-field and total text
limits. This version supports standard symbol kinds 1–26. Its 1 MiB serialized
JSON-value budget includes discarded extension fields; the separate transport
frame limit still bounds incoming bytes. An oversized or malformed response fails as a whole; narrow the query.
The existing transport frame and request deadlines remain in force. Cedar does not
request lazy symbol resolution, library class-file content or arbitrary server commands.

Choose a row to navigate through the existing workspace-root URI resolver and an
ordinary file read. Foreign/non-file locations cannot bypass that resolver. Dirty
open buffers are reused with their Undo history, while stale query/session/navigation
replies cannot take over newer user intent. A recorded range can be stale against
a dirty draft; it is not a claim that the draft and server index share a revision.

After Search or Enter, the query keeps keyboard focus. Press **Down** to select
the first openable result without opening it. With a result focused,
**Up/Down/Home/End** moves the selection and scrolls it into view; **Enter** opens
that exact row. Navigation stops at the ends and skips disabled non-file targets.
Tab and Shift+Tab retain normal control traversal. In the query, Up, Home, End
and modified keys retain their text-editing behavior. An active IME composition
does not transfer focus to results. Editing the query or dismissing the view
clears selection; reopening does not repeat the search or reopen a result.
These keyboard controls add no provider, automatic query or automatic file read.

The feature works with the normal Java recipe and the same provider in an explicitly
started Maven leaf session. Maven's existing scope and trust limitations still apply.
This is type navigation, not general method search, a recursive filesystem index or
refactoring. No new background index or process is added by Cedar; the running JVM
and its existing JDT index retain their normal resource cost.

Upstream behavior: [JDT LS 1.61.0 workspace symbol handler](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/v1.61.0/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/WorkspaceSymbolHandler.java).
