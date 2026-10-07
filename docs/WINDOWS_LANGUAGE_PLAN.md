# Windows language-service ownership plan

This is the next language milestone, not an enabled capability. Windows LSP stays
unavailable until its exact public revision passes native Windows runtime tests.
The existing Linux/macOS transport uses blocking process pipes and detached I/O
threads; adding a Windows process launcher alone would not make blocked writes
cancellable or give complete ownership of their lifetimes.

## 1. Cancellable stdin primitive

Extend cedar-winprocess with an explicit piped-stdin constructor, retaining NUL
stdin for ordinary tasks. The parent owns an overlapped outbound named pipe and
one bounded pending write. Its event, OVERLAPPED and buffer stay alive until
completion is established. Polling exposes partial counts; cancellation is a
request followed by completion, never permission to free an active buffer.

Retain the existing current-logon DACL, first-instance/local-only pipe rules,
peer verification, exact three-handle inheritance list and Job assignment before
resuming the suspended root. Do not call FlushFileBuffers on a pipe as a shutdown
shortcut: it can wait for the peer to consume data. Explicit close is distinct
from cancel-and-complete. Drop terminates the owned Job and completes every
stdin/stdout/stderr operation before freeing resources.

Runtime gates include a blocked full pipe, short/complete writes, peer EOF/exit,
repeated close/cancel, handle stability, simultaneous independent launches and
exact known-descendant termination. OS console support processes may contribute
to live Job counts; cleanup still requires the entire owned Job to reach zero.

Contracts: [WriteFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-writefile),
[CancelIoEx](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelioex),
[FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers),
[process attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).

## 2. Owned language transport and agent integration

One joined Windows worker owns the process, incremental Content-Length decoder,
write state, bounded queues and stderr tail. Stop has independent control and
cannot queue behind full outbound data. Enforce the existing message/request/
event limits and account retained bytes before allocation. Cover fragmented
headers/bodies, multiple frames and partial-frame EOF.

An expired unsent frame can be dropped. A fully sent request with a late reply can
be cancelled/discarded according to the protocol. A partially transmitted frame
that times out or fails poisons the connection: kill the owned process tree and
start a fresh session, never replay an uncertain operation or reuse broken frame
boundaries. Root exit or stdout EOF is terminal; stderr EOF alone is not.

Gate Windows language execution on the isolated agent and separate workspace
trust. Tasks and LSP have separate Jobs; concurrent startup must use exact handle
lists everywhere. Test stopping either independently and agent death while both
are live. Keep Git, legacy synchronous Run and DAP disabled until their own
ownership paths are implemented. Jobs do not own pre-existing build daemons or
processes launched by unrelated brokers.

The agent currently serves requests sequentially. LanguageStart can spend up to
60 seconds initializing; the transport client allows 75 seconds. A joined I/O
worker does not let file reads, task cancellation, Stop or peer EOF overtake that
handler. The first integration must disclose this bound, or separately introduce
an asynchronous start/status/stop-while-starting protocol and its capability gate.
Ordinary synchronous language queries also block agent scheduling for their
request deadlines. Do not promise responsive startup cancellation without that
additional state machine.

## 3. Real Windows Java acceptance

Launch an explicit absolute java.exe with literal arguments, one Equinox launcher
JAR, config_win and a unique writable data directory. No batch parser, PATH
fallback or debugger listener is needed. Use pinned official JDT LS and Java
artifacts, verify their hashes and notices, and keep them as test dependencies
rather than bundling them in Cedar. The historical Linux JDT results do not count
as Windows acceptance.

In a synthetic Eclipse project, verify diagnostics, completion and deferred
resolve/imports, hover/definition, correction of an error, source bytes unchanged,
shutdown and restart, Unicode/spaces in paths, and full owned-process cleanup.
Control transport/JVM environment in the test parent without mutating a running
multithreaded agent's global environment. Record the public source SHA, JDK build,
JDT artifact digest/server version and cleanup evidence. Missing dependencies,
compile-only checks and ignored tests are not runtime passes.

See [language services](LANGUAGE_SERVICES.md), [process ownership](WINDOWS_PROCESSES.md)
and [feature priorities](FEATURE_MATRIX.md). Authenticated SSH interoperability
remains a separate remote-development acceptance gate.
