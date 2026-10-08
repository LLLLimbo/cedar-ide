# Java Organize Imports preview

Cedar offers an explicit action for the active Java buffer. Start the configured
typed Java language service, open a Java file, and choose **Organize imports**.
The action requires the agent capability and an acknowledged Standard JDT LS
1.61.0 session advertising `java.edit.organizeImports`. This is a compatibility
check, not authentication of the selected language-server binary. Existing
workspace execution trust remains required.

This checkpoint supports the shipping typed Java route on the isolated Windows
agent. Generic Linux/macOS LSP sessions do not enable this action. Agent
capability checks keep older or unsupported connections usable without sending
the command.

Cedar synchronizes the captured unsaved draft before requesting import edits.
Review the proposal and choose Apply to change only that draft in one Undo
transaction. Cancel leaves it unchanged; Save remains separate. Typing,
switching documents, reconnecting or restarting the language service invalidates
stale requests or previews. Empty/no-op results do not create Undo history.

The server may sort imports, remove unused imports and add imports that resolve
uniquely. Ambiguous missing types remain unresolved: Cedar does not choose a
candidate for you. “No import edits returned” does not prove the file is correct.
Classpath contents, index readiness and malformed Java can affect the proposal.
The request uses the existing bounded language-request lifecycle and can wait
for indexing; it does not certify that background indexing has settled.

## Deliberate limits

- Current acknowledged Java document only, with its exact synchronized version
- One fixed advertised command and one agent-generated document URI
- No caller-selected command/arguments or folder/project import organization
- Only ordinary text edits for that same document
- No file-resource operations, additional documents, annotations or commands
- No server-initiated apply, automatic Save or organization on Save

Returned URI spellings are checked against the acknowledged document, without
using the returned URI to open another file. Invalid paths, unsupported result
shapes, overlapping edits, invalid UTF-16 ranges or bounded-size violations are
rejected before any draft change. The feature does not implement general
WorkspaceEdit, project-wide refactoring or rename.

## Pinned implementation basis

The [fixed command](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/commands/OrganizeImportsCommand.java)
computes import edits. Cedar keeps `workspace.applyEdit=false`, which makes the
[delegate return the proposal](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/JDTDelegateCommandHandler.java#L67)
rather than request client application. The command disables the interactive
chooser; the [pinned import operation](https://github.com/eclipse-jdt/eclipse.jdt.ui/blob/54bd63e6453a7eb5db0bfc1d60f17e9568095619/org.eclipse.jdt.core.manipulation/common/org/eclipse/jdt/core/manipulation/OrganizeImportsOperation.java#L740)
leaves ambiguous choices unresolved. No general execute-command route is exposed.

Actual acceptance for this checkpoint is tracked in [the verification report](TEST_REPORT.md).
