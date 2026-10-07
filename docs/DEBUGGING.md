# Debugging foundation (0.2.0)

Cedar now has a separate `cedar-debugger` crate with a real, tested **stdio DAP
process transport**. It is not wired into the desktop UI, workspace bridge, SSH
agent, or remote debug launching. There is no debugger toolbar, breakpoints UI,
stack/variables panel, Java adapter integration, or automatic adapter installation.

The existing `cedar-language::dap` module still owns the DAP envelope types; the
new crate reuses those types and the bounded Content-Length framing parser. DAP
responses are correlated by `request_seq`, never JSON-RPC `id` or adapter `seq`.

## Lifecycle and sequencing

An explicitly configured executable is spawned directly, without a shell, with
stdin/stdout carrying DAP and stderr drained into a bounded output tail. The
adapter inherits the current environment. Executing an adapter/debuggee is trusted
code execution, not a sandbox, and launch arguments may themselves cause code to
run. Generic transport requests are not an authorization boundary.

`request` returns a `RequestHandle` immediately after bounded queue admission.
Handles can be polled with `try_result` (which consumes an available result) or
consumed with `wait`. Responses and events are read independently. The correct
launch sequence is:

1. Send `initialize`; inspect its response capabilities
2. Send `launch` and retain its pending handle
3. Receive `initialized` without waiting for the launch response
4. Send and await breakpoint/configuration requests
5. Send `configurationDone` if the adapter advertised support
6. Await the outstanding launch response, then handle stopped/continued/exited/
   terminated events and request threads, stacks, scopes and variables as needed

Blocking on `launch` before configuration can deadlock adapter startup. This crate
makes asynchronous sequencing possible; it deliberately does not impose an IDE
session state machine or choose launch configurations for callers.

`disconnect(terminate_debuggee)` requests adapter-managed termination with the
configured shutdown deadline, then stops the direct adapter regardless of success.
`stop` and `Drop` kill/reap the direct child and are idempotent. Dropping an
individual request handle only cancels local interest; it neither sends DAP cancel
nor undoes adapter-side effects. A timeout likewise does not establish whether an
operation ran, so effectful requests must not be blindly retried.

## Bounds and failure behavior

Defaults (all configurable) are 8 KiB headers, 8 MiB messages, 128 pending requests,
64 outbound messages, 256 critical events, a 64 KiB combined UTF-8 output tail,
15-second request deadlines and a 1-second disconnect deadline.

- Outbound JSON is size-limited while serializing, before writer-queue admission
- The bounded writer queue prevents stalled adapter stdin from blocking request
  submission, deadline delivery, or direct-child stop
- A watchdog expires each request from its original submission deadline, even if
  nobody is waiting on that handle
- Successful and failed responses must match the pending request's command;
  mismatches terminally invalidate the transport
- Late, canceled, duplicate and unknown response IDs are ignored and counted,
  without maintaining an unbounded historical-ID set
- Adapter output is kept as a byte-bounded UTF-8 tail with a discarded-byte count;
  output floods do not fill the critical event queue. The current tail flattens
  output categories and adapter stderr; it is not a structured output console
- All non-output events are treated as critical. A full event queue causes an
  explicit `EventOverflow` terminal fault and fails all pending requests. There is
  no silent dropping of stopped/continued/terminated events while pretending the
  session is still valid
- EOF, malformed JSON/envelopes, truncated/oversized frames and transport errors
  fail outstanding requests. Terminal state is retained outside the event queue
  and delivered once as `Event::Closed`, with `terminal_error()` always available.
  Queued events are invalidated on a terminal fault; callers must discard cached
  session state rather than interpreting earlier queued events as live state
- Every reverse request is rejected with a DAP failure response, including
  `runInTerminal` and `startDebugging`. No adapter-supplied commands are executed.
  If that refusal cannot enter the bounded writer queue, the transport fails

The direct child is supervised and reaped. **Whole-process-tree cleanup is not
implemented or guaranteed.** A launcher can create a new process group; killing
only the adapter or its group cannot prove the debuggee exited. A descendant can
also inherit a pipe and keep a blocking reader/writer alive; stop deliberately
avoids joining such pipe workers, so those worker threads can remain until the
inherited pipe closes. This is another reason not to expose arbitrary adapters
through the UI/remote bridge yet. A production integration needs platform-specific
containment and observable cleanup, not a process-group-only assumption.

## Reproducible tests

Run deterministic tests without installing a debugger:

```sh
cargo test -p cedar-debugger --features test-server
cargo clippy -p cedar-debugger --all-targets --all-features -- -D warnings
cargo check -p cedar-debugger --all-targets --all-features \
  --target x86_64-pc-windows-msvc
```

The feature-gated `cedar-mock-dap` Rust binary tests real process pipes, deferred
launch, configuration, breakpoints, threads/stack/scopes/variables, continue,
disconnect, out-of-order replies, reverse requests, output floods, event overflow,
malformed/oversized/truncated messages, EOF, command mismatches, deadline expiry,
late replies, canceled handles, pending caps, invalid commands, outbound size
limits, adapter-reported errors, stalled stdin and repeated stop/drop reaping.

The real integration test is opt-in and Linux-only. It neither installs packages
nor searches for an interpreter. Create an isolated environment from official
PyPI and explicitly select it:

```sh
python3 -m venv /path/to/debug-tools
/path/to/debug-tools/bin/python -m pip install --index-url https://pypi.org/simple \
  --only-binary=:all: --no-deps debugpy==1.8.22
CEDAR_DEBUGPY_PYTHON=/path/to/debug-tools/bin/python \
  cargo test -p cedar-debugger --test debugpy_real -- --ignored --nocapture
```

The synthetic fixture installs a 15-second OS-default SIGALRM safety limit before
its breakpoint. The Linux test harness additionally acts as a child subreaper,
records owned process identities, checks PID start times before cleanup, and
kills/reaps only its owned synthetic descendants if needed. **These test-only
safeguards are not production containment.**

### Verified on 2026-10-07

On Linux x86-64 with Python 3.12 and Microsoft debugpy 1.8.22:

- A real breakpoint at fixture line 13 was verified; stopped reason was breakpoint
- Real `threads`, `stackTrace`, `scopes` and `variables` returned the paused fixture
  frame and local `answer = 41`
- Continuing printed `CEDAR_ANSWER=42` and emitted termination
- Graceful disconnect while paused, forced adapter crash and forced client drop
  were each exercised. No owned process remained running after the 3-second
  observation window in these specific runs, before fallback test cleanup
- Two live owned TCP listeners observed at the paused point were loopback-only;
  DAP socket events were checked during startup too
- Windows MSVC all-targets/all-features compile check passed; **Windows runtime
  behavior was not tested**. This crate has no workspace/UI integration and does
  not bypass Cedar's current local-Windows process-tool guard

The installed wheel was
`debugpy-1.8.22-cp312-cp312-manylinux_2_34_x86_64.whl`; its downloaded SHA-256 matched
[official PyPI metadata](https://pypi.org/pypi/debugpy/1.8.22/json):

```text
8a697acec45dbc70d17fb5d9f4f61989fc294d273c69de487fd10cb35fdd75eb
```

The installed package reported version `1.8.22`. It is an isolated external test
dependency, not bundled with Cedar. Other wheels/platforms have different hashes.
The crate source itself adds only the existing workspace serde/serde_json/thiserror
and the local cedar-language dependency (plus tempfile for tests).

## Important debugpy networking finding

The test launches `python -X frozen_modules=off -m debugpy.adapter` **without
`--port`**, using `console: "internalConsole"`, `redirectOutput: true`,
`subProcess: false`, and `debugAdapterHost: "127.0.0.1"` in its launch request.
Nevertheless, debugpy 1.8.22 creates sockets. In addition to its internal loopback
server/launcher communication, it opens an extra loopback **client listener** in
stdio mode and reports this with `internal: false` in `debugpySockets` events.
This is not just an internal-only socket arrangement. Its
[versioned adapter entry point](https://github.com/microsoft/debugpy/blob/v1.8.22/src/debugpy/adapter/__main__.py)
calls `clients.serve` before setting up the stdio client; the
[client implementation](https://github.com/microsoft/debugpy/blob/v1.8.22/src/debugpy/adapter/clients.py)
labels that listener separately from internal listeners.

All observed endpoints were 127.0.0.1, not wildcard interfaces, but loopback-only
is not equivalent to authenticated access or same-user isolation. Do not forward
these ports or claim socket-free, authenticated, or production-safe remote debug
operation. The extra client-listener behavior and descendant containment must be
reviewed before promoting this transport to UI/workspace integration.

## Java boundary

The available Eclipse JDT Language Server installation does not include the
Microsoft Java Debug Server plugin. That standard adapter's unauthenticated
wildcard listener is unsuitable for the intended remote design without additional
containment/authentication work. This milestone does not install or expose it,
and does not claim Java debugging. Python verification proves the DAP transport
foundation only; Java remains a separate integration project.

### Upstream option review and next security work

The v1.8.22 adapter `--help`, its complete argument parser, the client constructor,
and Microsoft's [API reference](https://github.com/microsoft/debugpy/wiki/API-Reference)
were checked. No documented switch disables the extra client listener while
keeping this stdio launch mode, and no DAP-client authentication switch was found.
`--host` selects a bind address; `--port 0` requests an ephemeral port rather than
turning networking off. The token switches describe debug-server authentication,
not a client-authentication gate in `clients.Client`. The upstream
[README](https://github.com/microsoft/debugpy/tree/v1.8.22#readme) explicitly warns
that anyone able to connect to a debugger port can execute code in the debuggee.

A minimal future, reviewed **IDE-stdio-only** adapter variant would avoid calling
`clients.serve` at all when no client TCP port is requested (or add an explicit
`--stdio-only` mode), keep endpoint reporting valid without a client listener,
and reject flows that assume such a listener exists. For the initial narrow
integration, keep subprocess debugging disabled and reject attach, reverse
terminal launch and secondary sessions. Its tests must verify absence of a client
listener throughout startup/launch/shutdown, not merely hide socket events, and
retest the internal server/launcher loopback and authentication boundaries.
This would still have internal sockets and still need OS process containment.

No installed adapter source was patched and no firewall or host security setting
was changed. A reproducible, hash-pinned reviewed fork or an upstream supported
option is required before using that design; this document is a proposed next
step, not a claim that the current dependency is stdio-only or production-safe.
