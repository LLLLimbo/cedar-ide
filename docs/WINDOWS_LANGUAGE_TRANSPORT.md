# Windows language transport checkpoint (disabled in the IDE)

This slice implements the Windows `StdioRpc` backend only. The workspace Windows
LSP rejection and capability gates remain unchanged. Primitive stdin and this
transport both need independent review and exact-commit native Windows runtime
results before integration may enable language services in the isolated agent.
A successful MSVC cross-target check is compilation evidence, not runtime proof.

## Ownership and facade

The existing `StdioRpc` request, notification and event APIs stay intact. Common
code validates configuration and routes JSON-RPC messages. Non-Windows builds
retain the existing blocking pipe reader/writer behavior and direct-child
cleanup. Windows compiles a different backend using `cedar-winprocess`, with one
joined worker that exclusively owns `WindowsCommand`, incremental decoding,
write progress and stderr retention.

The controlled launcher requires an absolute UTF-8 native `.exe`, literal UTF-8
arguments and an existing absolute working directory. Omitting the directory
uses the current directory at launch. No PATH/PATHEXT lookup, shell expansion,
installation or language-server discovery is added. The environment is inherited.
`process_id` is the cached creation identity, useful only for diagnostics; all
cleanup uses owned handles and the private Job. The host must separately control
all spawning and enforce trust, as described in [Windows processes](WINDOWS_PROCESSES.md).
The standalone transport cannot validate or manufacture that host guarantee.

The Windows worker drains stderr regardless of `inherit_stderr`. When true it
keeps only the last 16 KiB and includes that tail in terminal diagnostics; when
false it discards the bytes. It never writes to the host stderr, which could
otherwise block transport and shutdown. This is the sole platform-specific
change to the meaning of that existing option.

## Bounds, deadlines and outcomes

Configured outbound, pending-request and event capacities are enforced by the
shared bounded queues/router. The workspace's existing configuration is 1 MiB
per message, eight queued writes, eight pending requests and 32 events. This
slice does not change those settings or activate them on Windows. Library
callers can use their own finite `ClientOptions` limits; overflow in the derived
payload budget is rejected before launching a process.

A queued outbound payload reserves at most its content limit. There is only one
additional active payload, one small Content-Length header, and one primitive
stdin buffer of at most 64 KiB. The incremental decoder holds either a bounded
header or a body bounded by the validated Content-Length. It does not allocate
the advertised body up front or collect decoded frames into an unbounded batch.
The primitive has two fixed 8-KiB capture buffers and supplies at most four chunks
per stream per round. The worker keeps no stdout transcript. JSON object/node
allocation and queue bookkeeping add bounded per-payload overhead, and a terminal
stderr diagnostic has a fixed additional bound; this is not an OS memory quota.
Returned values, caller-owned inputs and simultaneous caller-local serialized
payloads awaiting admission are outside this queue-retention accounting. This
is not a total process-memory bound under unrestricted concurrent callers.

`request_timeout` also bounds assembly of each incoming frame on Windows. Its
clock starts at the first received byte and does not reset when another byte
arrives. Idle time at a complete frame boundary is unlimited. A longer per-call
initialize timeout does not lengthen this independent frame assembly limit.
Partial headers/bodies at EOF, malformed framing/JSON and over-limit messages
terminate the connection. Stderr EOF alone is harmless; stdout EOF is terminal. Root exit permits at most
250 ms of routing buffered or pending output to EOF after terminating owned
descendants. It never waits for a descendant to exit naturally and never submits
further stdin writes.

Each outgoing deadline covers queueing and every write chunk. An expired frame
that the worker observes expired before submission is discarded. A notification
whose caller times out before receiving its acknowledgement conservatively aborts
the connection even if it may still be queued: this API cannot prove delivery
state to that caller. Once submission starts, a timeout,
write error or uncertain completion poisons the connection, terminates its Job,
and joins cancellation. No part of that frame is replayed. Completed notifications
mean the complete frame was accepted by the transport, not that the server
processed it. Once a request was fully transported, a response timeout removes
its pending ID, tries to enqueue `$/cancelRequest`, and discards late responses;
that timeout alone need not poison a healthy connection.

Stop uses a separate atomic flag and thread wake, so it cannot queue behind
outbound frames. Each worker turn services bounded output plus at most one
write submission/completion. Ordinary idle polling is five milliseconds. Stop,
Drop, stdout EOF, root exit and all terminal errors terminate the Job, cancel and
complete stdin I/O, drain final output for at most 250 ms, complete both capture
operations and wait for the root. The worker is joined before the owner returns
from abort/Drop/finish. Kernel termination or cancellation can delay cleanup;
this is not a hard wall-clock guarantee and live I/O storage is never freed to
satisfy a timer. Panic unwinding retains process cleanup and signals completion.

## Agent scheduling is a separate gate

The current agent still handles protocol requests sequentially. `LanguageStart`
may wait up to 60 seconds for initialization, while its process-client deadline
is 75 seconds. A peer `LanguageStop`, task cancellation, file operation or peer
EOF cannot overtake that handler. Ordinary synchronous language queries also
delay other requests for their own deadlines. A joined transport worker does not
change this scheduling contract or make initialize cancellable by another agent
protocol request. Asynchronous start/status/stop integration, if required, is a
separate reviewed protocol slice.

## Validation

Deterministic framing, timeout-state, byte-accounting and routing tests run on
Linux as well as Windows. Existing language-process regressions exercise the
portable backend on Linux. The controlled Windows fixture suite is opt-in:

```sh
cargo test -p cedar-language --all-features --test windows_transport -- --ignored --test-threads=1
```

The twelve cases include warmup plus two eight-session batches measuring host
handles/threads after joined cleanup, with fixed small runtime-bookkeeping
allowances and independent fixture-lifetime checks. They require an actual
Windows runner and its built Rust fixture. No language
server download is involved. Native results must be recorded against the final
source commit; ignored or skipped tests do not establish a runtime gate. Future
activation additionally needs agent-level concurrent task/LSP startup, independent
stop, owner-death and real Java language-service coverage. The private transport
checkpoint does not advertise any of those capabilities as passed.
