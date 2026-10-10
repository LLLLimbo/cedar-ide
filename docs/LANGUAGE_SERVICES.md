# Language services · current transport contract

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

`cedar-language` is a Rust stdio LSP client library, integrated with the native
editor through the workspace agent, using frontend/agent protocol **4**. Formatting,
references and document symbols use explicit bounded operations. Deterministic mock-process
tests and a real JDT navigation/formatting probe exercise these paths. The
[verification report](TEST_REPORT.md) separates candidate checks from final
aggregate and native-window acceptance.

Java/JDT and an older, deprecated community Kotlin server have historical real
semantic checks; the inspected official Kotlin package remains blocked on its
license/setup requirements. Server installation is explicit and external. The
separate `cedar-debugger` crate has DAP transport but no integrated debugger UI.
No general refactoring or complete Java/Kotlin IDE compatibility is claimed.

## Working APIs

- `LspClient::spawn(ProcessConfig, ClientOptions)` starts exactly the supplied
  executable and argument vector. It does not interpolate a shell command.
- `initialize(root_uri, initialization_options)` exchanges capabilities and sends
  `initialized`. It retains static capabilities and rejects an incompatible
  position encoding. Failed initialization closes the process.
  `initialize_with_timeout(root_uri, options, timeout)` gives cold starts their own
  deadline without changing normal completion/hover request deadlines.
- `did_open(uri, language_id, version, text)`, `did_change(uri, version, text)` and
  `did_close(uri)` track open state and monotonically increasing versions.
- `completion`, `definition` and `hover` take a document URI and `Position`.
  They check negotiated capabilities and return the complete JSON result, including
  nulls, completion lists/arrays and definition links. They do not apply edits,
  execute completion commands or render server-provided markup.
- `resolve_completion(item)` resolves a selected original completion item only
  when `completionProvider.resolveProvider` is true. Initialization advertises
  lazy `documentation`, `detail` and `additionalTextEdits`. The original item,
  including opaque `data` and extension fields, is sent intact. The server may
  remove its `data` in the result; retain client-side document/version context
  separately. This call uses the normal request deadline and frame limits and
  never executes an attached command or applies edits.
- `formatting(uri, tab_size, insert_spaces)` requests plain document edits after
  checking static `documentFormattingProvider`, open-document state and indentation
  width 1–16. It returns JSON without applying it. The workspace operation also
  requires an exact synchronized version; LSP itself has no formatting version field.
- `references(uri, position, include_declaration)` checks `referencesProvider`
  and returns the raw unversioned `Location[]`/null result. The frontend synchronizes
  all matching open drafts and confines every navigation target through the agent.
- `document_symbols(uri)` checks `documentSymbolProvider` and requests hierarchical
  `DocumentSymbol[]` or flat `SymbolInformation[]`. Initialization advertises
  hierarchical support; the frontend preserves whichever shape is returned.
- `next_event(timeout)` returns typed `PublishDiagnostics`, other notifications,
  unsupported server requests, overflow notices, or closure. This is independent
  of waiting requests. A poll timeout is `None`, not an EOF indication.
- `request`, `request_with_timeout` and `notify` allow explicit protocol extensions
  after initialization; callers own the extension's capability checks and types.
  Lifecycle and tracked document methods have dedicated entry points.
- `shutdown()` performs the shutdown/response/exit sequence, then waits briefly,
  then requests owned cleanup. `shutdown_with_outcome()` distinguishes protocol
  completion from actual process/I/O evidence. Windows reports Job-backed cleanup;
  Linux reports its private process group, root exit code or signal, root reaping,
  released parent endpoints and worker join separately. Other portable hosts
  retain their legacy direct-child behavior. A legacy `Ok` can follow forced
  cleanup and is not evidence of graceful exit.

`StdioRpc` exposes the lower-level JSON-RPC transport without the LSP state machine.
Do not pass DAP envelopes to it: DAP has a different request/response structure.

### Buffer synchronization and coordinates

The editor always supplies the entire new buffer. A full-sync server receives a
full replacement. An incremental-sync server receives one edit over the previous
whole-document range. The library tracks that range's end in UTF-16 code units,
including supplementary characters and CRLF/LF/CR line endings. It retains the
previous version and end position, not another copy of the document text.

Positions are zero-based UTF-16. Native UI cursor offsets must be converted from
UTF-8 bytes/scalar positions before requests. Correct percent-encoded document
URIs belong to the server filesystem. For a remote session, `/workspace/a.kt` on
the agent and a local cache path are different identities. The library does not
rewrite URIs or fetch remote files.

### Concurrency and failure contract

Share a client through `Arc<LspClient>`. Issue feature requests on background
workers, with one independent consumer draining events. The reader routes numeric
request IDs, so responses may arrive out of order and notifications do not wait for
an outstanding request. Windows and Linux use an owned nonblocking I/O worker;
other portable hosts retain separate blocking workers. Existing request/write
and shutdown-grace deadlines are unchanged. Linux partial incoming frames also
use the request deadline; an idle frame boundary does not time out.

Linux fixes a three-second cleanup observation deadline at the first cleanup
trigger, shared by finish, abort and Drop. Normal completion requires actual
worker return/join and root reaping. Timeout is cached as unverified/not joined;
the same owner retains eventual wait responsibility, with no replacement watcher
or caller PID retry. This is an observation budget plus ordinary scheduling and
thread finalization, not a hard bound on an uninterruptible process. Parent pipe
endpoints close before eventual wait; inherited stderr remains inherited.

Exclusive child-wait ownership is required: no competing SIGCHLD reaper,
SIGCHLD=SIG_IGN or SA_NOCLDWAIT. Root status is observed with WNOWAIT
before group signaling, and the root remains unreaped until signals finish.
Lost wait ownership disables all further cached-PID operations. Group signaling
is not an independent group-empty observation and cannot contain escaped or
credential-changed descendants. Abrupt agent death is not covered. Unverified
Linux generic cleanup blocks new language startup in that workspace. Reconnecting
does not prove the previous server has exited. Basic typed Java also uses this ownership contract on Linux isolated agents.
Linux InProcess retains generic LSP. The 0.39.1 Linux isolated Maven extension passed exact Ubuntu native acceptance; Windows retains its existing Maven route.
The Linux typed Stop body tags the platform and reports a bounded exit code or
signal; Windows retains its original u32 exit-code body. Use a matching frontend
for Linux typed Stop. Old frontends reject the unfamiliar shape safely.

Lifecycle transitions are exclusive. Shutdown waits for already-running operations
(up to their own deadlines); no operation can race a tracked notification past the
shutdown transition. Document changes are serialized and versions advance only
after their notification is written. A successful write is not a server-side
processing acknowledgment.

Timeouts remove pending requests and queue a best-effort cancellation. Late replies
are discarded. A notification write timeout aborts the session because delivery is
uncertain. EOF, invalid JSON, malformed envelopes and bad framing wake pending
callers with an error. Create a fresh session after a terminal transport error.
The library does not automatically restart a server or replay editor buffers.

Default limits are 8 KiB headers, 8 MiB JSON bodies, 64 queued writes, 256 queued
events, and 128 pending requests. Request timeout is 10 seconds; exit grace is one
second. All are configurable through `ClientOptions`. There is also a 4,096 open
document cap, 16 KiB URI limit and 256-byte language-ID limit. Outgoing serialization
stops at the configured body limit. Queue bounds are counts, not a total memory
budget: many maximum-size messages can still consume substantial memory. Tune
limits for the target machine and benchmark real workloads before claiming savings.

When an event queue fills, the reader continues routing responses and counts lost
events. The consumer receives `Lagged { dropped }` before its next poll. Invalidate
cached diagnostics, discard the remaining stale queue, and restart/reopen if a
complete diagnostic snapshot is needed. Never silently present an old diagnostic
snapshot as current. Also discard versioned diagnostics older than the buffer's
current version in the UI.

All server-initiated requests receive method-not-found automatically, plus a
visible event. In particular, `workspace/applyEdit` never silently modifies user
files. Dynamic registration, workspace/configuration callbacks, work-done progress
creation, file-watch registration and multi-root management are not implemented
or advertised. An external server may need more client features before it works
well with this subset.

Stderr is inherited by default and may be discarded explicitly; it is never an
undrained pipe. A server must keep stdout protocol-clean. The child inherits its
parent environment. Only launch trusted, explicitly configured programs: this
library is not a sandbox, credential filter, process-tree manager or CPU/memory
limiter. Drop terminates the direct child, not every descendant. Reader/writer
threads are not synchronously joined, since an unrelated descendant could retain
a pipe. A production remote agent should add process-group supervision.

## Phase-4 editing and navigation contract

Start a trusted server from **Language** and select the matching language profile.
**Format preview** first synchronizes the active draft, then opens read-only
Before/After text. **Apply** revalidates its captured connection, server session,
request, document identity, source text and edit version before one draft-only
undo transaction. **Cancel**, Escape or closing the window discards the proposal.
Tab switches, edits, close/reopen, reconnect, restart and newer requests invalidate
it; cursor-only movement is allowed, and Apply maps the latest cursor. Shortcut-only Undo/Redo batches skip cursor-only history states; mixed input
keeps native event order. The saved baseline/revision and project disk remain
unchanged until a separate save.

The shared [plain edit planner](TEXT_EDITS.md) accepts only `TextEdit[]`/null,
up to 1,024 edits and independently 1 MiB source/result/inserted text. It rejects
unsupported fields, annotated/resource edits, commands, overlap, ambiguous
insertions, invalid UTF-16 positions, surrogate splits and CRLF interiors before
any mutation. Null, empty and text-identical results do not add undo history.
Formatting does not grant recovery ownership over an older unreviewed backup.

**Find references** has an explicit **Include declaration** option. All matching
open drafts must finish synchronization before dispatch; participant identity,
text, edit version and acknowledged LSP version are captured and checked. A
changed participant set or query position invalidates the pending response.
Results remain labeled an **unversioned server snapshot**: synchronization is
not proof of unopened-target freshness, and targets can change after the query.
Only `Location[]`/null is accepted, with at most 1,024 locations, 16 KiB per URI
and 512 KiB aggregate retained URI text. Locations are never applied as edits.

**Refresh outline** is explicit, with no automatic symbol index. Hierarchical
`DocumentSymbol[]` preserves nesting and navigates with `selectionRange`; flat
`SymbolInformation[]` stays flat and retains its container as display context.
Mixed/hybrid results fail. Up to 2,000 nodes, depth 32 (root depth 0) and 512 KiB
retained text are accepted. Hierarchical source ranges, selection containment
and child/parent containment are checked. Unknown numeric kinds get a generic
label. Editing invalidates the outline.

References and flat outline navigation use `LanguageResolveUri` on the agent;
no frontend assumption converts a remote server URI into a local file. Navigation
reuses dirty open targets unchanged and validates ranges against their current
text. Selecting a local outline item supersedes older pending URI resolutions or
file opens so a late reference result cannot override a newer navigation choice.
See [frontend details](../crates/app/PHASE4_LANGUAGE.md) and
[rename safety boundaries](REFACTORING_ROADMAP.md).

The workspace agent uses a 60-second initialization deadline and normal
10-second LSP request deadlines. These differ from any enclosing transport/test
deadline. A timeout is shown; normal UI requests are not retried automatically.
A successful didOpen/didChange transmission acknowledges the client's tracked
version, not completion of the server's indexing work.

## Native UI and remote-agent integration

1. Keep the process/client on the machine that owns the workspace. Put remote
   launch behind the agent's explicit command-execution permission.
2. Initialize off the render/event thread and show errors plus negotiated features.
3. Open/change/close documents through these APIs. Debounce changes and preserve
   strictly increasing versions. Keep unsaved text on the owning editor side.
4. Schedule completion/hover/definition and explicit formatting/navigation requests asynchronously. Resolve a selected
   completion before acceptance if the server advertises a resolve provider, so
   lazy import edits are available. Preserve the original item for the resolve
   call. Reject stale UI results if the document/caret changed while a request
   was running. Validate UTF-16 ranges and nonoverlapping primary/additional edits,
   then apply them atomically against the captured document version. Commands are
   separate from text edits and must never be executed implicitly.
5. Drain diagnostic events independently and map server URIs to existing workspace
   files only after validating the workspace boundary. Treat text and URLs as
   untrusted display content; never execute server-suggested commands implicitly.
6. Expose transport closure/lag to the user and offer an explicit restart. Close the
   service on workspace disconnect and application exit.

A small manual interoperability probe is included:

```sh
cargo run -p cedar-language --example lsp_probe -- \
  /absolute/path/to/server \
  file:///absolute/workspace \
  file:///absolute/workspace/Main.kt \
  kotlin /absolute/workspace/Main.kt --stdio
```

The arguments after the source path are passed to the server verbatim. Replace
paths, language ID and server options for the installed release. The probe checks
initialization/open/hover/close/shutdown and samples events briefly; it is not a
benchmark or a comprehensive language-server certification.

## Java and Kotlin configuration examples

These examples are explicit operator configuration, not bundled servers or an
installer. The pinned Java smoke below is the extent of verified external-server
interoperability. External servers and build tools may still require a JVM even
though Cedar's frontend and agent are Rust.
Moving them to the remote workspace can move that load off the local machine;
it does not eliminate it.

### Java: Eclipse JDT LS

Set `ProcessConfig.program` to the installed JDT LS launcher. Example argument
vector: `['-configuration', '/absolute/cache/jdtls/config', '-data',
'/absolute/cache/jdtls/project-a']`; set `working_directory` to the project root.
Use a distinct writable data directory for each workspace. The upstream launcher
wraps the required Java options. The documented runtime minimum is Java 21 and the
Python launcher needs Python 3.9; verify requirements against the chosen release.
For stdio, ensure `CLIENT_PORT` and `CLIENT_HOST` are not set in the inherited
environment. Direct Java invocation is also possible using exact launcher-JAR and
platform-configuration paths, without shell wildcards. See the
[official JDT LS launch and connection instructions](https://github.com/eclipse-jdtls/eclipse.jdt.ls#running-from-command-line-with-wrapper-script).

Initialize with the project `rootUri`, then use language ID `java`. Maven/Gradle
imports may need longer deadlines and additional callbacks. The smoke below
validated a tiny Eclipse project's import and semantic services. Maven/Gradle
imports, production builds, refactorings and debugging remain untested.

### Kotlin: official Kotlin LSP

An illustrative configuration is `program = '/absolute/kotlin-lsp/kotlin-lsp.sh'`,
`args = ['--stdio']`, with the Kotlin project directory as `working_directory`.
Use language ID `kotlin`. The upstream editor example documents `--stdio`; follow
the installed release's `--help`. Current source marks this wrapper deprecated in
favor of `bin/intellij-server`, so resolve the launcher for your release rather
than assuming the example path exists. Sources:
[official CLI installation](https://github.com/Kotlin/kotlin-lsp#install-kotlin-lsp-cli),
[official repository's stdio example](https://github.com/Kotlin/kotlin-lsp/blob/main/scripts/neovim.md#stdio-way),
[launcher migration notice](https://github.com/Kotlin/kotlin-lsp/blob/main/scripts/kotlin-lsp.sh).

The upstream project describes itself as alpha and based on IntelliJ components.
Cedar makes no claim that using Kotlin LSP eliminates JVM/IntelliJ-related runtime
costs. Actual support, startup time, project import and memory use must be measured
with the chosen release. The older fwcd server is
[marked deprecated upstream](https://github.com/fwcd/kotlin-language-server), so it
is not the default example.

## Historical phase-2 Java smoke (2026-10-07)

A Linux cloud test used the official Eclipse JDT LS **1.61.0** milestone archive
`jdt-language-server-1.61.0-202609031315.tar.gz`, verified against Eclipse's published
SHA-256: `338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64`.
The archive's initialize response identifies itself as `JDT Language Server
(Standard)` / `1.61.0-SNAPSHOT`; that is the server's exact reported version, despite
the milestone download name. Sources: [official archive and checksum index](https://download.eclipse.org/justj/?file=jdtls/milestones/1.61.0)
and [pinned Java 21 runtime requirement](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/v1.61.0/README.md#requirements).

Runtime: installed OpenJDK `21.0.12.1` on Linux, with `-Xmx512m`. No `javac` binary
was installed; JDT's semantic analysis still worked. The heap setting limits the
Java heap, not total process RSS. An independent measured rerun is reported below.

A fresh, temporary Eclipse Java project, classpath and source file were generated
by the opt-in `java_smoke` example. No user repository or remote machine was used.
The server was started directly through Java, with Gradle/Maven imports disabled
in initialization settings and no build scripts in the synthetic fixture.

Verified results:

- Initialize returned after **4,768 ms** and advertised incremental document sync
- Open produced a real semantic error: `Type mismatch: cannot convert from String to int`
- Hover resolved the local variable's `String` type and `Main.main(String[])` scope
- Completion returned `greeting : String`, including the server's text edit
- Definition resolved the local variable's declaration range
- A full-buffer `did_change` was translated to a whole-range incremental edit;
  the type error disappeared, leaving only an unused-local warning
- Close, shutdown protocol exchange and direct-child cleanup completed; the whole
  run took **11,234 ms**. Process cleanup can use the configured kill fallback

This is one fresh-process sample on one small fixture, not a benchmark. A
ten-second initialize budget was sufficient here, but a longer configurable
startup deadline is prudent for slower hosts or larger projects. The test's
request deadline is 60 seconds. No unsupported server callback occurred in this
run. Completion commands returned by the server were recorded, never executed.

The captured JSON-lines result is checked in at
[`crates/language/tests/evidence/jdtls-1.61.0-smoke.jsonl`](../PUBLICATION.md#verification-evidence).
Its temporary fixture URIs document that historical run and are not persistent
workspace paths. Reproduce with an already downloaded, verified distribution:

```sh
cargo run -p cedar-language --example java_smoke -- /absolute/path/to/jdtls /absolute/path/to/java
```

The example creates and removes its own temporary fixture and server data. It
exits nonzero unless actual semantic diagnostics, hover, expected completion,
definition and corrected diagnostics all succeed. No server archive is bundled
in this repository. Large workspaces and Gradle/Maven imports still need independent real-world
validation. Separate historical Kotlin and debugpy checks are described in
[KOTLIN_VALIDATION.md](KOTLIN_VALIDATION.md) and [DEBUGGING.md](DEBUGGING.md).

### Historical independent JVM memory sample

The same official archive, OpenJDK runtime, `-Xmx512m` setting and seven-line
synthetic Java fixture were run again on 2026-10-07. This repeated run used a new
JVM, temporary project and server-data directory; the host and operating-system
file caches were not reset. It passed the same semantic checks. Initialize took
**3,354 ms**, and the full rerun took **9,329 ms**. Times exclude Cargo compilation.

`java_smoke` now reads `/proc/{client.process_id()}/status` at three checkpoints.
It samples the directly launched **JDT LS JVM only**, separately from the frontend,
agent, Rust smoke runner, and any descendant processes:

| Checkpoint | Elapsed | VmRSS | VmHWM reported at checkpoint |
| --- | ---: | ---: | ---: |
| After initialize | 3,354 ms | 260,248 KiB / 254.15 MiB | 260,248 KiB / 254.15 MiB |
| After hover, completion and definition | 5,695 ms | 496,908 KiB / 485.26 MiB | 496,908 KiB / 485.26 MiB |
| After corrected diagnostics, before shutdown | 6,238 ms | 537,272 KiB / 524.68 MiB | 537,272 KiB / 524.68 MiB |

These are per-process Linux accounting samples, not Java-heap usage or proportional
set size (PSS). RSS includes native JVM overhead and resident mappings, so exceeding
the 512 MiB Java-heap cap is possible. `VmHWM` is the process's resident high-water
mark through each reading, not a separately measured whole-session peak after
shutdown. Linux notes that its RSS accounting is asynchronous and approximate;
see the [kernel's process-status documentation](https://docs.kernel.org/filesystems/proc.html#process-specific-subdirectories).

The independently measured frontend sample does **not** include this JVM. These
runs were not measured together; adding their readings would not establish a
concurrent application total. This small-project sample does not establish memory
use for larger Java/Kotlin projects, warmed long-lived sessions or concurrent
workspaces. No savings versus another IDE are claimed.

The complete rerun output, including explicit memory scope, exact KiB values and
all semantic checks, is saved in
[`jdtls-1.61.0-memory-smoke.jsonl`](../PUBLICATION.md#verification-evidence).
The earlier functional capture remains unchanged. A missing or malformed Linux
memory counter fails the measurement rather than reporting zero; other platforms
emit an explicit unsupported-measurement record. The parser has two focused tests:

```sh
cargo test -p cedar-language --example java_smoke
```


### Historical lazy completion-import resolution

The optional `--resolve-imports` mode reran the same official JDT LS 1.61.0
milestone and synthetic project on 2026-10-07, with the same JVM options. All
previous semantic checks passed. In addition:

- The initial `GregorianCalendar - java.util` completion had an ordinary primary
  text edit and opaque `data`, but no `additionalTextEdits`
- `resolve_completion` sent that original item back with its opaque fields intact
- The resolved response supplied `import java.util.GregorianCalendar;` followed by
  two newlines as an additional edit at line 0, character 0
- The primary edit and label remained unchanged; JDT removed its opaque `data`
  from the response, which the client permits
- The advisory `java.completion.onDidSelect` command was retained as data and never
  executed. The probe inspected returned edits without applying this candidate to
  the fixture; native UI edit application requires separate validation

The full run took **10,858 ms**. This proves lazy import retrieval, not the UI's
atomic edit application or a performance improvement. The original functional
and JVM-memory captures remain historical and unchanged; this extra resolve
operation is not an equivalent memory workload for cross-run comparison.

Evidence:
[`jdtls-1.61.0-completion-resolve.jsonl`](../crates/language/tests/evidence/jdtls-1.61.0-completion-resolve.jsonl).
Reproduce with:

```sh
cargo run -p cedar-language --example java_smoke -- /absolute/path/to/jdtls /absolute/path/to/java --resolve-imports
```

Capability negotiation follows the
[official LSP completion/resolve specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#completionItem_resolve).
The [official JDT LS resolve handler](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/main/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/CompletionResolveHandler.java)
conditions additional-edit resolution on the advertised client support. Mock
process regressions cover negotiation, absent/false providers, input/output shape,
opaque-field preservation, error responses, deadlines, notifications during a
pending resolve, frame-size bounds and absence of command execution.

## Phase-4 real JDT agent probe (2026-10-07)

`scripts/jdt_navigation_smoke.py` runs the actual stdio workspace agent against
installed JDT LS 1.61.0 (reported `1.61.0-SNAPSHOT`) and a fresh synthetic Eclipse
project. The captured run passed **10 checks** in **13.236 seconds**:

- Synchronized two unsaved Java drafts; references included one declaration and
  two calls from the second unsaved buffer
- Returned a real class with two method outline entries
- Returned 12 plain formatting edits, interpreted in UTF-16, preserving Chinese
  text; a synchronized formatted draft was idempotent
- Rejected a stale expected formatting version before contacting the server
- Rejected an outside-workspace navigation URI
- Closed documents and stopped the service; original Java source disk bytes were
  unchanged and the temporary fixture was removed

Evidence: [jdt-navigation-phase4.json](../PUBLICATION.md#verification-evidence). This agent
probe does not exercise the native preview, Apply or editor undo implementation;
the independent native checks are recorded in [TEST_REPORT.md](TEST_REPORT.md).
The final Linux release window separately passed real JDT preview/Escape/Apply
and single-step Undo/Redo after cursor and tab navigation, with unchanged Java
source disk bytes. That session used warmed server data; it is not another cold-
start or memory benchmark.

The first cold attempt failed on the normal **10-second LSP references deadline**;
a clean rerun produced the passing capture. The capture contains no readiness
retry entry. The smoke script now permits explicit retries of read-only references
within a **40-second semantic-readiness window**. Its outer agent response limits
are normally **20 seconds**, or **75 seconds for language startup**, and do not
extend the inner LSP deadlines. This test-only readiness behavior is not automatic
UI retry or a performance/stability claim.

```sh
python3 scripts/jdt_navigation_smoke.py \
  target/debug/cedar-agent /absolute/path/to/jdtls \
  /absolute/path/to/jdt-navigation.json /absolute/path/to/java
```

No JDK/server is bundled or downloaded by this script. It does not execute
returned commands or `workspace/applyEdit`. This small fixture is not acceptance
of production Maven/Gradle projects or semantic rename.

## DAP: honest current boundary

`cedar_language::dap` supplies typed envelopes and bounded framing.
`cedar-debugger` separately provides asynchronous process transport, response/event
routing and launch handles. Phase 2 used real debugpy for a Python breakpoint,
stack/scope/variables, continue/output and termination; these are retained
historical transport checks, not an integrated native or remote debugger.
Java debugging, debugger UI, adapter/project configuration, reliable general
process-tree cleanup and listener security remain outside the implemented
end-to-end path. See [DEBUGGING.md](DEBUGGING.md) and the
[official DAP overview](https://microsoft.github.io/debug-adapter-protocol/overview).
The shared Content-Length header alone does not make DAP interchangeable with LSP.

## Verification and protocol references

```sh
# Framing, capability/UTF-16 and DAP-envelope unit tests
cargo test -p cedar-language
# Adds a compiled Rust mock peer and actual subprocess protocol tests
cargo test -p cedar-language --features test-server
cargo clippy -p cedar-language --all-targets --features test-server -- -D warnings
```

The integration suite covers successful lifecycle and feature results, full and
incremental synchronization, malformed frames/JSON/responses, EOF and broken-pipe
races, server errors, incompatible capabilities, concurrent out-of-order responses,
notifications while requests wait, deadline recovery, event backpressure, bounded
pending requests, blocked stdin, unsupported server callbacks and direct-child
cleanup. It requires no external language server, JVM, SSH host or network secret.

The implementation targets the documented LSP 3.17 subset and UTF-16 coordinates;
it does not claim every feature of this or later revisions. Primary references:
[LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/),
[canonical protocol source](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.17/specification.md),
[DAP specification](https://microsoft.github.io/debug-adapter-protocol/specification).

The 0.41 Linux GUI Local route uses its fixed matching sibling agent; embedded Client::Local remains generic. This desktop route is pending current-checkpoint acceptance.
