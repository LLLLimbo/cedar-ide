# Remote transport validation and remaining gates

## Scope and current evidence

Remote development is a core path. A local backend or UI test is not evidence of
SSH authentication, remote process cleanup, or recovery after a real network loss.
The checks below exercise actual child processes and the compiled agent. They
use disposable files and synthetic tools, not user servers or credentials.

Linux results from 2026-10-07:

- `cedar-client` unit tests: real-pipe failure coverage plus strict SSH argument
  validation; the standalone Rust fault peer has portable source, with Windows
  execution pending exact-commit CI
- `cedar-agent` unit tests: failed write acknowledgements and sequential errors
- Compiled-agent integration: file revision conflicts and traversal rejection,
  concurrent file editing during a task, cancellation, reconnect isolation,
  orderly EOF, stdout BrokenPipe, malformed input, and forced termination
- OpenSSH 10.0p2 accepted the generated argument list using `ssh -G -F /dev/null`
  without contacting a server; this only verifies local option parsing

This document does **not** claim an authenticated SSH pass. No test SSH keys,
credential grants, server instances, or network listeners were created for these
checks. Windows runtime results for the new changes must be recorded against the
exact new CI commit, not inferred from a previous phase's successful CI.

## Repeatable entry points

```sh
cargo test -p cedar-client -p cedar-agent --all-targets --locked
cargo clippy -p cedar-client -p cedar-agent --all-targets --locked -- -D warnings
cargo build -p cedar-agent --locked
CEDAR_AGENT_BIN="$PWD/target/debug/cedar-agent" \
  cargo test -p cedar-client --test stdio_roundtrip --locked -- --ignored
```

On Windows, set `CEDAR_AGENT_BIN` to the built `cedar-agent.exe`. The existing
`scripts/verify.sh` and Linux/Windows CI explicitly run the ignored
`stdio_roundtrip` entry point. Its process-group assertions are Linux-only;
ordinary agent file round trips and synthetic transport fault tests are portable.

The fault peer is `crates/client/tests/fixtures/transport_peer.rs`, compiled once
per test executable through the installed `rustc`. It has no extra dependencies,
network access, shell requirement, or shipped binary target. Short deadline tests
use private seams, leaving the public request limits unchanged: 30 seconds for
ordinary operations, 75 seconds for language startup, and the bounded requested
run time plus ten seconds for the legacy synchronous Run operation.

## Fault matrix

| Fault or behavior | Evidence and expected result |
| --- | --- |
| Protocol 3 / 5 Hello | Connection rejected; exact protocol 4 remains required |
| Malformed Hello / missing root / wrong payload | Connection rejected; no usable client returned |
| Wrong response ID, including stale later ID | Session poisoned; reconnect required; no retry |
| Truncated frame / missing newline | Bounded reader returns incomplete-frame error |
| More than 8 MiB frame | Reader fails closed without unbounded allocation |
| Oversized outgoing request | Writer rejects before sending a mutation |
| EOF before Hello / between requests / after request | Disconnected result; no automatic replay |
| 4 MiB stderr flood | Pipe drained; only latest 4 KiB retained for diagnostics |
| Unsolicited response flood | Single response slot; close releases a blocked sender |
| Peer stops reading stdin | Request deadline remains effective; background forced close |
| Stalled response | 100 ms test deadline, session closed; production limits unchanged |
| Write committed but acknowledgement lost | Unknown-outcome warning; exactly one write recorded |
| New process connection | Fresh request counter and queues; no old requests copied |
| Agent write acknowledgement gets BrokenPipe | First write may already exist; subsequent queued write is not executed |
| Application error | Matching error returned; next request remains usable |
| Running task and file editing | Read/write remain available; busy gate rejects second task |
| Reconnect with old task ID | Poll/cancel returns unknown before any new task is started |

Task IDs are only unique within an agent process. A fresh agent may allocate the
same numeric ID again. The frontend's connection generation must remain part of
task/event identity and must discard events from older generations. The transport
does not claim a server-issued session nonce or task adoption/resumption.

## Lifecycle correction and proven boundary

The old `ProcessClient::drop` closed its request sender and immediately killed
its direct child. For a local agent that could bypass `Workspace` destructors;
for SSH it could cut off the opportunity to forward orderly stdin EOF.

The client now creates a dedicated child owner/reaper at connection time. Close
and transport failure drop the request sender and response receiver, then notify
that owner. An idle writer closes child stdin. The reaper gives the child up to
two seconds to exit normally, then terminates and reaps **only its direct child**.
No close-time thread creation or child wait runs on the caller/UI thread. The
request and response queues each have one slot; a stalled writer cannot build an
unbounded queue. A blocked pipe write is released when the direct child exits.

The two seconds bound the graceful opportunity, not an unconditional operating
system guarantee: an uninterruptible child, detached descendant holding pipe
handles, exhausted OS resources, or abrupt frontend process death can prevent
normal completion. Reader threads are not guaranteed cancellable against a
malicious process that escapes ownership and keeps inherited descriptors open.
The reaper does not chase arbitrary descendant PIDs or promise remote cleanup.
The client requires exclusive ownership of its child's wait status; it abandons
signaling after a non-interrupted wait error rather than risking a reused PID.
Do not install a competing SIGCHLD reaper or SA_NOCLDWAIT for these children.

Linux compiled-agent tests verify:

1. Normal client drop returns promptly; the agent observes EOF, drops its task
   manager, stops the ordinary task group, and reaps its task leader
2. Agent input EOF returns success with an active task cleaned up
3. Agent stdout BrokenPipe, truncated input, and oversized input return failure
   with active tasks cleaned up
4. A separately tested SIGKILL of the agent leaves its task running until the
   fixture's own two-second lifetime ends; forced death is **not** cleanup

Descendants are checked as no longer running. Any orphan descendant zombie is
reaped by its new parent/init; the suite does not claim the agent reaps processes
it never directly owned. A deliberately escaped process is outside the guarantee.
No real-network interruption cleanup claim follows from these local tests.

`main` already calls `run()` and only calls `process::exit` after `run` returns.
Consequently, normal `serve` errors unwind the workspace first. The tests verify
that behavior rather than replacing it with an unnecessary shutdown protocol.
Rust documents that `Child` is not automatically killed/reaped on drop and that
`try_wait` is nonblocking; `process::exit` itself does not run stack destructors.
See [Rust Child ownership](https://doc.rust-lang.org/std/process/struct.Child.html)
and [Rust process::exit](https://doc.rust-lang.org/std/process/fn.exit.html).

All transport failures retain the warning that an outcome may be unknown and a
write must be reloaded before retry. Neither reconnection nor cleanup replays
writes or commands. A timeout is not evidence a mutation was rolled back.

## SSH argument policy

`Command::args` supplies literal local arguments. Only the remote POSIX command
is shell-quoted, including spaces, apostrophes and shell metacharacters in roots
and executable paths. Host option injection, control characters, relative roots,
and port zero are rejected.

The client enforces batch mode, strict host keys even for localhost, no host-key update/IP recording,
no key addition to the authentication agent, no delegated GSSAPI credentials,
no tunnel/agent/X11/port forwarding, no local command, and no multiplexed
connection reuse. It overrides inherited settings that would detach SSH, close
stdin or suppress the command session. User-controlled routing such as
ProxyJump/ProxyCommand remains supported; this is not a sandbox for SSH config.
[OpenSSH option semantics](https://man.openbsd.org/ssh_config.5)

`StdinNull`, `SessionType` and `ForkAfterAuthentication` first appeared in
OpenSSH 8.7. Only these three aliases are listed in `IgnoreUnknown`, before their
overrides, so clients from OpenSSH 7.6–8.6 that cannot inherit them can still
parse the arguments. Existing `RemoteCommand` support already implies an
OpenSSH 7.6 minimum; older versions are not supported by this argument set.
[OpenSSH 7.6 release notes](https://www.openssh.org/txt/release-7.6)

Required security settings are never ignored. The explicit list takes precedence over a
user-configured IgnoreUnknown list; shared configs relying on additional unknown
platform-only settings can therefore fail closed and need compatible host
configuration. This is compatibility design; old-client runtime coverage is still
needed. [OpenSSH 8.7 release notes](https://www.openssh.org/txt/release-8.7)

## Next authenticated SSH gate: permission required

Prefer the no-listener OpenSSH inetd/ProxyCommand path described below. A separate
loopback listener is a fallback requiring its own explicitly approved scope.
No part of this section has been executed. Existing keys must not be read and
no existing authentication or account policy may be changed to make the test pass.

### Proposed narrow approval

> Allow authenticated OpenSSH regression tests in this project's cloud test
> folder using two temporary Ed25519 keypairs, isolated authorized_keys and
> known_hosts, and local `ssh`/`sshd -i` processes connected only by pipes. No TCP
> listener, user `~/.ssh` change, existing private key, or external server is used.
> Permit up to three regression runs in a 10-minute total window, then terminate/reap
> the test processes and destroy the temporary authentication files.

An explicit approval must precede key generation and authentication setup. Each
run also needs a fixed watchdog deadline, and the entire approved test phase
needs an agreed upper lifetime for the temporary keys; proposed maximum is ten
minutes total, with at most three passes and 120 seconds per connection.
Extending any limit requires asking.

### Exact temporary layout

Use `umask 077` and a new mode-0700 directory from
`<checkout>/target/ssh-validation.XXXXXX`; in this execution workspace the
absolute parent is `/path/to/rust-remote-ide/target`.
All ancestor ownership/modes must first be rechecked. Retain StrictModes.

- `host_ed25519` and `host_ed25519.pub`: disposable server identity
- `client_ed25519` and `client_ed25519.pub`: disposable test authentication
- `authorized_keys`, `authorized_keys_empty`
- `known_hosts`, `known_hosts_empty`, `known_hosts_wrong`
- `sshd_config`, `ssh_config`, `server.log`
- `bin/ssh`, `proxy-server`, `session-guard`, `fixture/`, `process-manifest`

Private keys and auth/config files are mode 0600. Never display private contents.
Construct `known_hosts` directly from `host_ed25519.pub` under the synthetic
hostname `cedar-stdio-test`. For a wrong-host-key case pin `client_ed25519.pub`
instead. Test authentication rejection against `authorized_keys_empty`, so no
third keypair is needed. No automatic acceptance, ssh-keyscan, or global store.

### No-listener execution design

A test-only `bin/ssh` shim executes `/usr/bin/ssh -F <D>/ssh_config "$@"`.
Only the dedicated test process receives this PATH. This exercises the normal
`Client::connect(ConnectionSpec::Ssh)` path and its strict flags unchanged.
The explicit `-F` isolates configuration; changing HOME alone is insufficient.
[OpenSSH command-line configuration](https://man.openbsd.org/ssh.1)

The synthetic host's ProxyCommand launches the deadline-bound `proxy-server`,
which execs `/usr/sbin/sshd -i -e -f <D>/sshd_config`. OpenSSH explicitly supports
`sshd -i` through ProxyCommand stdin/stdout; strict verification still uses the
requested synthetic host. This does not invoke the listener path; OpenSSH may
still use anonymous local socketpairs internally. Its own regression harness
uses this arrangement. [Upstream OpenSSH regression runner](https://github.com/openssh/openssh-portable/blob/master/regress/test-exec.sh)

 Pipe peers
have no trusted IP address, so do not use loopback address restrictions in
`authorized_keys`, `AllowUsers`, or Match rules for this mode.
[OpenSSH inetd operation](https://man.openbsd.org/sshd.8)

The isolated client configuration uses:

- `HostName 127.0.0.1`, `HostKeyAlias cedar-stdio-test`, `User agent`; port 22 is only metadata
- Only `<D>/client_ed25519`, `IdentitiesOnly yes`, `IdentityAgent none`, no certificate
- Only `<D>/known_hosts`, no global known-hosts file, KnownHostsCommand or DNS trust
- Only public-key authentication; no password, keyboard-interactive, hostbased or GSSAPI
- No hostname canonicalization; only the owned ProxyCommand, `ProxyUseFdpass no`
- No competing ProxyJump directive; isolated client subprocess uses `SHELL=/bin/sh`

The isolated server configuration uses:

- Only `<D>/host_ed25519` and `<D>/authorized_keys`
- `AllowUsers agent`, `AuthenticationMethods publickey`, `UsePAM no`
- Password, keyboard-interactive, hostbased and GSSAPI authentication disabled
- `StrictModes yes`, `DisableForwarding yes`, `PermitTTY no`, `PermitTunnel no`
- User rc/environment disabled; no authorized-key helper; bounded login grace/auth tries
- `ForceCommand <D>/session-guard`, verbose diagnostics directed to the test log

The authorized key has `restrict` plus an expiry no later than the approved
phase deadline. The forced-command guard compares `SSH_ORIGINAL_COMMAND` against
exact precomputed allowed strings, then execs an explicit argument vector. Do
not eval a merely prefix-matching string. Allowed commands are the compiled agent
on a disposable fixture, a deliberately missing executable/root, and explicitly
selected fault peers. A separate trusted-task fixture may run only the known
short-lived synthetic commands needed for cancellation and cleanup validation.
[Server authentication/session restrictions](https://man.openbsd.org/sshd_config.5)

First validate config with `sshd -t -f <D>/sshd_config` and inspect `ssh -G` using
only the test config. Non-root OpenSSH requires `UsePAM no` and serves this same
uid only. A locked account, missing runtime component or platform policy is a
blocker; do not change account state, system config, ownership or firewall rules.
[OpenSSH 10.0p2 session implementation](https://github.com/openssh/openssh-portable/blob/V_10_0_P2/session.c)

### Required assertions and cleanup

1. Successful encrypted/authenticated Hello, list/read/search, create/update,
   revision conflict, traversal rejection, and execution-trust enforcement
2. Real remote-shell quoting for paths containing spaces, apostrophes and shell
   metacharacters; no unrelated marker file created
3. Close/reconnect preserves saved fixture text and creates a new session;
   unknown command/write outcomes are not automatically replayed
4. Empty and wrong known_hosts reject before agent startup; no host store changes
5. Empty authorized_keys rejects noninteractively; no fallback authentication
6. Missing agent/root exposes bounded useful stderr diagnostics
7. Owned proxy/server interruption during a pending request disconnects the
   client; separately observe actual task cleanup and report its limits
8. Each run records case, exact binary revision, elapsed time, result, agent
   startup evidence, and owned process identity; stop after terminal evidence

Use an independent deadline supervisor and cleanup on normal completion, errors
and interruption. Close clients first, then stop/reap the owned proxy, server and
session processes using recorded ownership. OpenSSH's proxy cleanup alone is
insufficient because it sends SIGHUP without waiting for the proxy.
Verify no test process remains.
Destroy keys/configs/fixture files after the approved phase; keep only redacted
results. Avoid relying solely on a process group, because sshd sessions can make
new groups. Do not kill guessed or reused PIDs.

This pass proves the actual OpenSSH authentication/protocol path on this Linux
host. It does not prove TCP reconnection, packet-loss handling, external-network
behavior, jump hosts, Windows OpenSSH behavior, or production account policy.

### Separate loopback fallback

If inetd mode is blocked, report the precise reason and request approval before
using a listener. The bounded fallback is unprivileged sshd bound only to
`127.0.0.1:42222`, at most ten minutes per approved run, with the same isolated
keys/configs, exact command guard and cleanup. Stop if the port is occupied.
Use `[127.0.0.1]:42222` in known_hosts and add a loopback source restriction to the
authorized key. Do not alter system SSH/network settings or listen externally.

## Additive Hello design assessment (not implemented)

Do not loosen exact protocol-4 validation just because agent product versions
change. Product version and wire compatibility are different values. An optional
Hello metadata extension can be additive without changing the base operations:

```json
{"id":1,"result":{"Ok":{"type":"hello","protocol":4,"root":"/fixture","agent_version":"0.5.0","capabilities":["workspace.files.v1","tasks.async.v1"]}}}}
```

Suggested validation bounds: version at most 64 ASCII bytes; at most 32 unique
capability names of at most 64 ASCII bytes each; capability alphabet
`[a-z0-9._-]`. Reject malformed recognized metadata, tolerate unrecognized future
fields/names, and retain the outer 8 MiB frame cap. Agent version is informational.
Capabilities describe implementation, never execution trust or permission.

Missing metadata means a legacy protocol-4 peer; an explicitly empty capability
list is distinguishable. Keep base protocol-4 features available as documented;
gate only future optional operations on positive capability advertisement.
Unknown capabilities are not errors and never enable unimplemented client code.

Preferred coordinated change: add optional/defaulted Hello fields to the shared
protocol type, populate them in Workspace, retain them in Client, and expose
bounded connection information to the UI. The old typed reader already ignores
additional fields. If shared types must stay frozen, private serde_json wire
wrappers in agent/client can inject/extract the same metadata, at the cost of
additional conversion and validation; do not implement both approaches.
[Serde unknown/default-field behavior](https://serde.rs/container-attrs.html)

Required matrix before shipping: old/new client-agent combinations; missing and
empty metadata; malformed/oversized metadata; unknown future capability; wrong
protocol rejection; non-Hello/error frames unchanged; no capability bypass of
Workspace trust. A server session nonce, task adoption and reconnect replay are
separate designs and are not implied by this extension.
