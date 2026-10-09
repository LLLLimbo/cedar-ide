# Go to Java implementation locations

This feature requests `textDocument/implementation` explicitly from an already
running typed Java/JDT session. It requires connection execution trust, an agent
that advertises the operation and the server's actual `implementationProvider`.
It does not start a server, rebuild an index, save a document or execute a command.

Before dispatch, the current draft and participating open Java drafts must be
synchronized. The source version and UTF-16 cursor are captured with the session.
Returned locations remain unversioned index results: synchronization does not
prove index readiness or freshness of unopened target files.

Only ordinary locations are supported. Selecting a result uses Cedar's existing
workspace-root resolver and file-read path. Dependency archives, external paths
and unsupported URI schemes cannot be opened through this feature. Existing
dirty buffers retain their contents and Undo/Redo. Edits, newer navigation,
cancellation, reconnects and language-session changes invalidate stale requests.
Hiding tools or leaving this result view dismisses the captured navigation
snapshot, including any pending target open. Reopening does not revive it; make
another explicit query. Other tools' ordinary fields and results are unchanged.

## What JDT means by implementation

At the pinned server revision, querying a type returns subtypes, which need not
all be concrete classes. Querying an interface method declaration can return its
concrete implementing method declarations; an inheriting subclass without an override does not receive an
invented declaration location. Invocation sites can include the original
declaration, and super calls can resolve directly to a superclass method.
Therefore the results are implementation locations returned by JDT, not a full
runtime call graph or a promise to list every concrete implementation.

The pinned sources used for this contract are:

- [Provider advertisement](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/InitHandler.java#L179)
- [Standard implementation request](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/JDTLanguageServer.java#L1086)
- [Subtype and method collector](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/ImplementationCollector.java)
- [Actual upstream tests in the tests tree](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.tests/src/org/eclipse/jdt/ls/core/internal/handlers/ImplementationsHandlerTest.java#L68)

The tests file was verified at blob `02250bbfcd169b32dc2685df4d3dd165dafeec11`.
Source inspection establishes the intended contract; Cedar's actual supported
runtime behavior requires the exact-checkpoint acceptance described in
[the verification report](TEST_REPORT.md).
