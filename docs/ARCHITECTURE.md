# Architecture

The native UI owns draft text, tab identity, saved revisions, and connection generation. All connection and workspace requests execute on a dedicated worker. UI results carry a generation and request ID, so a stale connection cannot mutate the newly selected workspace. Save acknowledgements refer to the exact submitted snapshot and never replace newer typing.

`Client` selects an in-process Workspace for local files or a child-process transport for SSH. SSH starts the same `cedar-agent --root ...` that is used by integration tests. The agent is a sequential bounded-frame server. Each workspace contains one optional language server, so both filesystem paths and language-server filesystem access remain on the workspace machine.

## Failure semantics

- Transport errors close the session; a remote application error such as `conflict` keeps it usable
- A timed-out write has an unknown outcome. Reconnect/read before retrying; revision checking prevents blind overwrite
- Reconnect is explicit. It preserves drafts and old disk revisions, intentionally allowing a later save to detect remote changes
- Switching workspaces checks for dirty state both before connection starts and when the new connection completes
- The UI will not silently quit while a write or tool request is in flight
- Agent stdout is protocol only. Process diagnostics go to stderr and the client retains a bounded tail
- Unexpected / oversized / truncated wire frames fail closed

## Security model

This program is a developer tool, not a multi-tenant sandbox. A trusted workspace and its toolchain are permitted to execute arbitrary account-level code only after the user enables tool execution. Even Git's clean/process filters make GitStatus subject to this gate. Language server requests such as workspace/applyEdit are rejected rather than executed implicitly.

The filesystem layer rejects traversal, absolute paths, platform prefixes, special files and symbolic links. Save uses a temporary file in the same parent, fsync, late revision verification, and atomic replace or no-clobber create. Another process with directory-write access can race canonicalization or the final version check; use an OS isolation boundary for adversarial workspaces.

OpenSSH is responsible for authentication and encrypted transport. The client requires existing known-host trust, uses noninteractive BatchMode and explicit StrictHostKeyChecking=yes, disables automatic host-key updates, local commands, connection multiplexing, agent forwarding, X11 forwarding and port forwards. It does not manage secrets or listen on a network port. The remote command is quoted for a POSIX shell, and destination values cannot inject local SSH options.

## Extensibility

New workspace operations belong in cedar-protocol and Workspace; they automatically become usable through local and SSH transports. A protocol-version bump is required before shipping incompatible changes. Production evolution should add explicit feature negotiation, capabilities and request cancellation.

The LSP transport has separately bounded frames, outbound messages, pending requests and events. Notifications never block response routing; overflow is explicitly reported. UTF-16 positions and negotiated full/incremental change modes are tested. The desktop currently exposes this via a manual read-only panel, leaving edit application to a later transaction/undo layer.

DAP types and Content-Length framing intentionally do not reuse JSON-RPC routing: DAP uses request_seq and events rather than JSON-RPC IDs. No debugger capability is claimed until a complete adapter-backed workflow exists.

A later plugin host should be out-of-process with capability-scoped RPC, not arbitrary native libraries loaded into the UI. Project models, language servers, build tools and debugger adapters should remain off the UI thread and preferably agent-side.
