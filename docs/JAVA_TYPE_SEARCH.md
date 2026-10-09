# Find Java type

The Language panel can explicitly search the index of an already running, trusted
Java language session. Enter a type name or qualified-name pattern and press Search
(or Enter in the query field). The agent must advertise `language_workspace_symbols`
and the server must advertise `workspaceSymbolProvider`.

This uses standard LSP `workspace/symbol`. Pinned JDT LS 1.61.0 searches indexed
source types, including nested types, with its own pattern/camel-case matching.
Cedar does not start a server, save files, scan directories or issue repeated searches
when the query changes. Search results are an **unversioned index snapshot** and
can lag unsaved editor changes. An empty result does not prove that no such type exists.

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

The feature works with the normal Java recipe and the same provider in an explicitly
started Maven leaf session. Maven's existing scope and trust limitations still apply.
This is type navigation, not general method search, a recursive filesystem index or
refactoring. No new background index or process is added by Cedar; the running JVM
and its existing JDT index retain their normal resource cost.

Upstream behavior: [JDT LS 1.61.0 workspace symbol handler](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/v1.61.0/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/WorkspaceSymbolHandler.java).
