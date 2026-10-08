# Scoped Windows Java support

Cedar's Windows isolated agent has a Java/JDT-specific language route. It uses
an installed JDK and Eclipse JDT LS; these dependencies are not bundled or
automatically downloaded. The tested acceptance distribution is JDT LS 1.61.0
with Java 21. The 0.9.0 normal-agent production route passed [exact Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37727806345). Its Stop used verified forced cleanup after grace expired, not natural exit.

## Configuration

Open the Language panel on a Windows isolated-agent connection and choose
**Java / JDT LS**. Paths refer to the workspace host, including when the frontend
is on another machine. Supply:

- **Java executable:** an existing absolute ASCII path to native java.exe,
  such as `C:\Tools\jdk-21\bin\java.exe`
- **JDT distribution:** the installed distribution containing `config_win` and
  exactly one `plugins/org.eclipse.equinox.launcher_*.jar`
- **JDT data directory:** an existing writable per-project directory outside
  the opened workspace, such as `D:\Cedar Java data\my-project` for a workspace
  at `D:\src\my-project`

Distribution, workspace and data paths may contain spaces and Unicode. UNC and
device paths are outside this recipe. The executable restriction follows the
validated Windows Java launcher behavior. Use separate data directories for
independent projects and serialize reuse of one directory.

Starting Java requires the existing explicit workspace execution trust. Editing
configuration does not start a process or enable trust. An embedded Windows
workspace cannot run this route, and generic Windows language-server startup
remains unsupported. Old agents without `language_start_java` never receive the
new operation.

The agent builds fixed literal arguments and selects the distribution as cwd;
it does not use a shell, PATH lookup, a batch launcher or user-supplied JVM
argument text. The configuration/data locations are encoded file URLs. The
agent rejects injected Java/socket environment options rather than modifying
its process environment: CLIENT_PORT, CLIENT_HOST, socket.stream.debug,
JDK_JAVA_OPTIONS, JAVA_TOOL_OPTIONS and _JAVA_OPTIONS must be absent.

## Current behavior and limits

This scoped profile handles Java source diagnostics, hover, completion with
validated deferred import edits, source navigation and the existing supported
language operations. Java mode synchronizes Java documents only. Maven/Gradle
project import and JDK class-file viewing are unavailable in this profile.
Server commands and automatic workspace edits remain disabled. In particular,
the completion selection callback does not run; ranking feedback and automatic
signature-help follow-up are unavailable.

The Java process uses a 512 MiB maximum heap; that is not a measurement of total
process memory or an IDE-wide memory guarantee. The Rust frontend and Java
language server are separate processes.

Startup and queries execute off the painting thread, but the request queue and
agent remain sequential. Other queued requests, including Stop, wait for the
current operation. Java requests have a 60-second server budget and a 75-second
client budget; the ten-second grace window starts after the shutdown response and covers
exit notification plus process termination. Kernel cancellation and owner destruction can exceptionally delay
joining, so these are not hard guarantees for every cleanup syscall.

Stop reports verified natural exit separately from forced cleanup. A forced
outcome is shown explicitly and is never relabeled graceful based on exit code0.
Unverified cleanup or malformed stop evidence blocks UI restart and cancels a
pending window close, preserving the error for review; reconnect before another
Java start. Stop does not perform the acceptance fixture's extra indexing query.

Native GUI interaction and authenticated SSH remain independently unverified.
Normal agent/Client headless tests exercise the production route; the separate
native bundle suite covers sibling-agent discovery. These claims do not imply
full IntelliJ IDEA feature or plugin compatibility.


## Explicit diagnostic refresh

When Java diagnostics stop arriving, first inspect the Problems panel's active
file status. Pending, stale and unversioned results do not verify the current
draft. Even an empty list may be pending or unversioned. Editing, closing or
reopening a document, reconnecting, malformed event data and lost events must
not leave an older snapshot marked current.

For a trusted typed-Java session, synchronize the current `.java` draft with
**Sync now**, then explicitly choose **Refresh Java diagnostics**. This optional
operation requires a compatible agent and the vetted server identity
`JDT Language Server (Standard)` / `1.61.0-SNAPSHOT` (the Maven version reported
by the tested JDT LS 1.61 milestone). Other versions, Syntax mode and generic
language sessions do not enable it. The identity is compatibility evidence,
not authentication of distribution bytes.

The agent sends one official JDT `java/validateDocument` notification for the
already-open URI after checking the exact synchronized document version. It
sends no replacement source, save, close/reopen, arbitrary method or retry.
**Request sent** only acknowledges notification transmission. A matching
versioned diagnostic batch can establish that snapshot; JDT's unversioned
batches remain labeled unverified, even when they arrive after the request.
No notification response or causal link is assumed. Existing drafts, editor
Undo/Redo and file contents are preserved.

This is a best-effort explicit mitigation. JDT still schedules publication
asynchronously and may fail to publish; the earlier intermittent correction
failure remains unresolved. Starting with 0.21.1, release acceptance explicitly
separates the unchanged rapid-edit/60-second spontaneous-push verdict from the
supported user-triggered recovery workflow. A spontaneous timeout stays recorded
as a timeout. Only one explicit refresh may then be attempted within the existing
bounded fixture envelope; its exact synthetic source witness and all source/editor/
cleanup invariants must pass. Failed recovery still blocks release. A recovered
workflow is not reported as spontaneous success or an upstream fix. General
freshness of unversioned diagnostics is still unverified.
No background refresh loop or extra Java process is introduced; the existing
memory and shutdown limitations still apply.

The extension is defined by the [official JDT protocol](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/lsp/JavaProtocolExtensions.java#L157)
and [URI-only parameter](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/lsp/ValidateDocumentParams.java#L20).
Its [unversioned publication](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/BaseDiagnosticsHandler.java#L144)
limits what the UI can establish.


## Starting and cancelling Java

With the complete optional startup capability group, **Start server** begins a
background startup owned by the agent. **Starting Java server** means the server
is not yet ready for language queries; ordinary file reads, saves and task
controls remain available. Choose **Cancel startup** to cancel that exact owner.
A ready/cancel race does not activate another session or cancel a later one.

Startup has a fixed 75-second readiness envelope, including an initialize
response capped at 60 seconds and its final notification. Expiry requests
cancellation. **Cancelling** remains visible until real cleanup finishes; it may
outlast the readiness deadline. Do not treat the spinner disappearing, elapsed
time or a sent Cancel request as verified cleanup. Unverified cleanup blocks
another start on that connection and requires reconnecting. Cancellation does
not roll back work an external server already performed.

Only one startup is owned at a time. Closing/reconnecting must resolve startup
and the existing pending-save/draft safeguards. No start, edit or save is replayed
automatically. The implementation adds a transient owner, not another JVM.
Ready means the LSP initialization handshake completed; background indexing may
continue. Once Ready, language queries and ordinary Stop retain their existing
synchronous behavior and cleanup reporting, so a slow query can still delay
queued workspace requests. An older agent without all three startup
capabilities keeps the original synchronous launch; its notice explains that
Stop is available only after launch returns.

## Explicit import organization

For a supported Standard JDT session, **Imports → Organize imports** requests a
preview for the current synchronized Java draft. Apply changes only that draft
in one Undo transaction; Save remains separate. Ambiguous missing types are not
automatically chosen. Other-document/resource edits and unsupported results are
rejected. See [the full scope and limits](JAVA_IMPORTS.md).
