# Accepted 0.61.0 protocol reader

These are byte-for-byte source snapshots from accepted public commit
[`7ec8e2f13ce82c57185c705929c0c17c61fcbdfa`](https://github.com/LLLLimbo/cedar-ide/commit/7ec8e2f13ce82c57185c705929c0c17c61fcbdfa)
(workspace version `0.61.0`), before the scoped-search extension was added.
The original source bytes and Git blob identities are preserved:

| Source | Git blob |
| --- | --- |
| `crates/protocol/src/lib.rs` | `89e09e4afbf7e1377374fb44140e4e702361becd` |
| `crates/protocol/src/file_discovery.rs` | `9d9b519147b9cc17c65c015ddcfb475d72068af1` |
| `crates/protocol/src/maven_dependencies.rs` | `8707970b7d2ab118074b34e5090e654cc8e3212a` |

`../../scoped_search.rs` compiles this original source as a separate module,
including its actual enums, Serde derives, validators, frame reader, frame
writer, and original unit tests. The test crate aliases the fixture's original
`RemoteError` at its root to satisfy the two unchanged child modules. It does
not substitute current protocol aliases or copy a build pipeline.

The compatibility test sends a current Hello frame with both versioned
extensions through the accepted reader, checks preservation of all old agent
metadata and file-discovery metadata, exercises the old validation bounds,
and proves that the accepted operation enum rejects SearchIn.
