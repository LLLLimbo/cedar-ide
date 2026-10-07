# Remote capability discovery · protocol 4

Phase 6 adds a bounded, optional `agent` object to the existing Hello response.
This is support discovery within the existing wire protocol, not general
protocol-version negotiation, remote attestation, authentication or task recovery.
Exact protocol **4** is still required. Product version is informational.

## Wire shape

A file-only example, using the existing response envelope:

```json
{"id":1,"result":{"Ok":{"type":"hello","protocol":4,"root":"/work","agent":{"schema":1,"version":"0.6.0","os":"linux","arch":"x86_64","capabilities":["list","read","write","search"]}}}}
```

Capability names are the existing serialized operation names. `hello` requires
no capability. The object has required `schema`, `version`, `os`, `arch` and
`capabilities` fields. The outer object can be absent or null for an older peer.

Validation limits:

- Metadata schema must be 1
- Version is 1–64 printable ASCII bytes
- OS and architecture are 1–32 bytes using lowercase ASCII identifiers
- At most 32 unique capability names, each 1–64 bytes using `[a-z0-9._-]`
- Wrong types, missing required inner fields, duplicate recognized JSON fields,
  duplicate capability names, invalid characters and exceeded limits fail
- Unknown fields and well-formed unknown capability names are tolerated;
  unknown names do not add executable client behavior
- The existing 8 MiB frame bound still applies

Metadata validation is performed before accepting the handshake. A present but
invalid object never becomes a legacy connection, and no fallback handshake is
sent. An empty explicit capability list is distinct from absent metadata.

## Compatibility policy

| Frontend / agent | Behavior |
|---|---|
| New frontend / new metadata-aware protocol-4 agent | Use validated advertised support |
| New frontend / metadata-free protocol-4 agent | Allow list, read, write and search; require agent upgrade for Git, commands and language services |
| Old protocol-4 frontend / new agent | Old typed readers ignore the added field; existing backend checks still apply |
| Any unsupported wire protocol | Reject; product version never overrides wire compatibility |

The files-only legacy policy intentionally tightens the phase-5 behavior and
supersedes its earlier design sketch. It avoids guessing execution support from
SSH usage or the frontend operating system. It does **not** mean the old agent
was incapable of those operations; upgrade and reconnect to expose them again.
There is no fallback from asynchronous tasks to the older blocking Run operation.

List and Read are the minimum supported workspace. Without them, a new client
rejects the connection before installing it. Missing Write permits browsing and
local draft editing but not saving. Search is independently optional.

## Platform support and execution trust

Workspace advertises the implementation compiled into the backend, without
looking for Git, JDKs, language servers or executable files and without starting
a task supervisor or tool. Advertisement is identical with execution trust on
or off. Support is not availability of an installed third-party tool.

- Filesystem operations are advertised on supported workspace platforms
- GitStatus, legacy Run and asynchronous tasks currently use Linux/macOS
  process implementations and are omitted on other platforms
- Language startup follows its separate implementation guard, currently disabled
  on Windows. Non-Windows compilation is not native-platform acceptance
- No PTY/terminal capability is advertised; Cedar has no such protocol operation

The frontend uses capabilities rather than `frontend_is_windows || uses_ssh`
guesses. A Windows frontend can use a capable Linux backend. A Linux frontend
must not offer unsupported command startup against a Windows backend.

Effective execution still requires a ready connection, complete operation-family
support, explicit workspace execution trust and the existing action/session
checks. Backend trust checks remain independent. Metadata never turns on trust,
changes SSH arguments, accepts host keys, creates credentials or enables an
unknown operation. A malicious agent can lie about all reported metadata;
version/platform labels must be read as reported information, not verification.

## Lifecycle groups and optional language features

Starting an asynchronous task requires **RunStart, RunPoll and RunCancel**.
Starting a language session requires **LanguageStart, LanguageOpen,
LanguageChange, LanguageClose, LanguageEvents and LanguageStop**.
Shared protocol constants define these groups for client and UI checks.

LanguageQuery, LanguageResolveUri, LanguageResolveCompletion, formatting,
references and document symbols are individually optional. Existing language-
server capabilities remain an additional requirement. Missing navigation does
not disable an otherwise usable diagnostic/formatting session. Missing URI
resolution never permits a frontend URI-to-path fallback. Missing completion
resolution never bypasses a required resolve-before-apply path.

Accepted support is immutable within a connection. Poll/Cancel and language
shutdown remain available for a session started with the required support;
reconnect discards old tasks and language sessions rather than adopting them.

## Connection and error behavior

Client caches the **entire first validated Hello**, including root and metadata.
The worker reads that snapshot rather than issuing another wire Hello. Public
`request(Hello)` also returns the cache while connected; after a known transport
disconnect it returns the disconnected error. No server session token is added.

A missing capability is rejected locally before sending a request. This does not
poison the connection. A rejected RunStart is definitely not started, unlike an
acknowledgement lost after a transmitted mutation. Transport failures continue
to report potentially unknown outcomes and never automatically replay work.

The frontend associates accepted information with its current connection
generation and typed workspace identity. All Hello events, including errors,
require an active matching connection attempt. Late or duplicate events cannot
replace a Ready session. Canonical-root and recovery identity checks precede
installation; support is cleared when disconnecting or beginning a reconnect.

Controls and dispatch paths both enforce support. Profile Save checks Write
before serializing into the raw document or changing its form baseline. Blocked
saves retain editable drafts; queued actions recheck current state.

## Validation scope

Protocol tests cover the frozen old-reader/new-response shape, new-reader/legacy
shape, malformed metadata and raw duplicate JSON keys. Client tests use the
existing portable stdio peer to verify one wire Hello, cached roots, no-I/O
rejection and legacy behavior. Workspace tests cover actual compiled platform
sets and trust independence. UI tests cover lifecycle prerequisites, missing
Write, optional language features and stale/duplicate connection events.

`python3 scripts/capability_smoke.py target/debug/cedar-agent` exercises metadata
and trust boundaries through a real local stdio agent on a synthetic fixture.
It creates no SSH keys, listener or external connection. Actual authenticated
SSH and native trust-on validation remain separate permission-gated work; none
of these checks substitutes for them. See [remote faults](REMOTE_VALIDATION.md)
and the current [verification report](TEST_REPORT.md) for executed results.
