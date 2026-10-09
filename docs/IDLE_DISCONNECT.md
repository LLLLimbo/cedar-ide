# Passive idle connection-loss detection

Cedar observes EOF, malformed responses and pipe errors reported by its existing
stdio transport even when no user request is active. The frontend marks that
connection disconnected and retains open drafts, selections, Undo history and
owned recovery records. Reconnect remains an explicit action.

No heartbeat or additional wire traffic is sent. A peer that stays connected but
stops responding cannot be detected while idle by this mechanism. Existing request
deadlines still apply when an operation is sent. There is no automatic retry,
reconnect, task restart or replay of a possibly committed write.

A completed response is processed before a later terminal event. A lost response
to a write remains an unknown save outcome and can be inspected using the existing
explicit read-only reconciliation action. Current matching contents establish a
disk baseline, not proof that the original write committed or the physical file
identity stayed the same.

The event belongs to one connection generation. A late event from an old process
cannot disconnect a replacement. Capability and language-session state are cleared;
remote task or server cleanup cannot be inferred from a broken transport. The local
transport keeps its existing bounded cleanup and child reaper.

There are no protocol or capability additions. The controlled-pipe acceptance
uses a normal trust-off agent on generated files and explicitly closes its stdin.
This demonstrates observed local transport loss, not real SSH authentication,
network outage behavior, Windows GUI use or remote-process cleanup.
