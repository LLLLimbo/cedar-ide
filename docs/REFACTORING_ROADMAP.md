# Safe language editing and the rename boundary

Phase 4 implements explicit current-document formatting previews, references,
and document outline. These are independently useful; they do not establish a
safe general WorkspaceEdit or rename transaction.

## Current formatting contract

Only plain TextEdit[]/null is accepted. The source must be synchronized and its
exact LSP version must still match at the agent. The frontend captures document,
connection and language-session identity, source text and edit version. A
preview is read-only; Apply rechecks that identity before one native undoable
change to the draft. It never saves automatically or changes the saved baseline.
Cursor movement alone is harmless; typing, changing the active document,
reconnecting or restarting the language service invalidates the proposal.

Unknown edits, annotations, commands, resource operations, invalid UTF-16
positions, overlaps and limits are rejected before any draft mutation. Existing
older recovery copies remain protected by their normal ownership rules. See
[TEXT_EDITS.md](TEXT_EDITS.md) and
[frontend feature details](../crates/app/PHASE4_LANGUAGE.md).

## Why general rename remains deferred

1. A request must synchronize every participating dirty buffer, not only the
   declaration's file. Unversioned edits against another open draft can otherwise
   address stale text
2. Reading an unopened file after receiving an unversioned response does not
   prove that the server computed edits from those bytes
3. An LSP null document version is not proof that any currently dirty buffer
   matches. Disk-owned versus open-buffer-owned state must be tracked explicitly
4. URI aliases and repeated document entries require canonicalization before
   building a workspace-wide plan
5. Applying multiple drafts together does not supply cross-document atomic undo.
   Current native history is per document
6. A rename may also need file-resource operations. JDT's inspected ChangeUtil
   omits resource changes when the client does not advertise those operations.
   Merely rejecting returned RenameFile is therefore insufficient: a public-class
   rename can produce text edits without its required file rename. prepareRename
   alone does not rule this out

A future implementation needs an explicit snapshot protocol, preview of every
text/resource effect, conflict and rollback semantics, a coherent undo boundary,
and actual member/type/package rename regressions. It must not equate a valid
text-edit range with a semantically complete refactoring. A server remains a
trusted language tool, not a proof of source correctness.

## Primary protocol and implementation sources

- [LSP 3.17 formatting](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/language/formatting.md)
- [LSP references](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/language/references.md)
- [LSP document symbols](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/language/documentSymbol.md)
- [LSP versioned text document identifiers](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/types/versionedTextDocumentIdentifier.md)
- [JDT ChangeUtil](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/main/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/ChangeUtil.java)
- [JDT PrepareRenameHandler](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/main/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/PrepareRenameHandler.java)

Upstream implementation links are moving branches. The caveat describes the
code inspected during this development stage, not every past/future JDT version.
