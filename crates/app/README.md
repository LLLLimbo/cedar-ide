# Cedar native desktop frontend

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../../PUBLICATION.md#verification-evidence).

Cedar is a native Rust application using `eframe`/`egui` 0.31.1 and the Glow renderer. It does not embed a browser or a JVM. The remote workspace agent and optional language server run separately from the UI.

## Run

From the repository root:

```sh
cargo run -p cedar-app --bin cedar -- /path/to/workspace
```

The optional argument prefills the local workspace field. Select **Connect workspace** to open it. For SSH, enter the system OpenSSH destination, port, remote directory, and path to the installed `cedar-agent`. Authenticate and verify the host key in an ordinary terminal first. Cedar never silently accepts unknown SSH host keys.

## Editing and workspace tools

- Local/SSH connections, explicit reconnect, background requests, and draft retention after disconnection
- Directory navigation, new-file drafts, up to 32 tabs, and path-based quick open
- UTF-8 editing, line numbers, Java/Kotlin/Rust lexical colors, and case-sensitive find
- Revision-checked saves, disk-version inspection, and explicit confirmation before discarding a dirty tab or quitting
- Project text search with clickable line results
- Trusted-workspace Git status and explicit executable commands with literal argument rows, cancellable task output, and saved project profiles in `cedar.tasks.json`

Shortcuts:

| Shortcut | Action |
| --- | --- |
| Ctrl/Cmd+P | Choose an open buffer or current-directory file; explicitly open a relative path |
| Ctrl/Cmd+G | Go to a 1-based line in the current buffer |
| Ctrl/Cmd+F | Find and preview literal replacements in the current buffer |
| Ctrl/Cmd+S | Save |
| Ctrl/Cmd+W | Close the current tab |
| Ctrl/Cmd+J | Hide tools and return to the editor, or reopen the selected tool |
| Ctrl/Cmd+Shift+E | Hide tools and focus enabled Explorer Refresh without refreshing |
| Ctrl+Space | Request completion |
| F12 | Go to definition |
| Ctrl/Cmd+K | Show hover information |
| Ctrl/Cmd+Z | Undo |
| Ctrl/Cmd+Shift+Z | Redo |

The explorer's **R** button refreshes its directory; **Up** opens its parent.
Hiding tools retains their fields, results and filters and does not stop tasks or
language services. The tools **x** button also returns to the editor. Without an
open document, hiding tools uses an available sidebar anchor. Access shortcuts
yield to modal input and ignore mixed input batches rather than diverting text,
paste or another action. Holding the shortcut does not repeatedly toggle tools.
These shortcuts provide an alternative at small window heights; expanded forms
and crowded center controls still have separate responsive-layout limitations.

## Agent capabilities and trust

Each accepted connection retains the first validated handshake. The status bar shows the agent-reported version and OS/architecture; its tooltip summarizes feature support and the separate trust setting. These are unverified support claims, not identity or permission. Support follows the workspace agent's advertised operations, never the frontend OS or the SSH/local transport.

Listing and reading are required to connect. When writing is unavailable, buffers and profile forms remain editable but all saves are disabled; profile Save cannot change the raw editor or advance its baseline. Search can be unavailable independently. Commands require the complete start/poll/cancel lifecycle. Language sessions require start/open/change/close/events/stop; query, URI navigation, formatting, references, symbols, and completion resolution are gated separately, alongside the language server's own capabilities. A completion that requires resolution cannot bypass that step when the agent lacks it. Stop and Cancel stay available for active sessions.

Legacy protocol-4 agents without metadata retain file listing, opening, editing, saving, and search. Upgrade the workspace agent to enable Git, commands, or language tools. Capability claims never enable workspace trust; reconnecting, cancellation, and disconnection clear usable metadata until a new handshake is accepted.

## Native language features

1. Connect with trusted tool execution enabled
2. Open **Language** / **LSP**. On a capable POSIX agent, select **Installed stdio server** and enter its executable, JSON arguments and language ID. On the Windows isolated agent, select the dedicated **Java / JDT LS** mode and configure the installed Java/JDT paths and external data directory; generic Windows LSP is unavailable
3. Select **Start server**
4. Matching open files synchronize automatically, with a 350 ms editing debounce
5. Diagnostics appear in **Problems**. Click a location to jump to the file
6. Place the cursor on a symbol and press F12, Ctrl/Cmd+K, or Ctrl+Space

Only one server runs per connection. A Java profile syncs `.java` files, a Kotlin profile `.kt`/`.kts`, and Rust `.rs`. Servers and any required JDK must be installed separately on the workspace host. Starting a trusted server can index projects or execute repository code under that account.

The editor coalesces changes while a sync is in flight. Document-position queries and editor actions wait for their captured draft snapshot to synchronize. **Find Java type** instead queries the server index without synchronizing drafts; its unversioned results may lag behind edits. **Check Maven model** reports the imported on-disk root POM, not an unsaved POM draft. Document-specific requests are invalidated when their captured draft or cursor no longer matches. Stopped/restarted sessions and lost connections also invalidate their outstanding results. Diagnostics are polled at a bounded one-second cadence while a server is active; the UI does not run a perpetual 60 fps polling loop. Automatic updates can be switched off; **Sync now** and **Refresh events** remain available.

### Completion and imports

Ctrl+Space opens a candidate menu. Single-click selects an item; Enter, **Apply selected**, or double-click accepts it. When supported, Cedar resolves the item first to obtain lazy imports. It validates every edit against the captured text and applies the primary edit and additional same-document edits atomically. The change stays unsaved and is one undo step. Escape or the popup close button cancels a pending acceptance.

Cedar supports plain-text edits and explicit replacement-mode InsertReplaceEdit. Snippets, ambiguous/overlapping ranges, unknown command-dependent items, and unsupported complex edits are disabled with an explanation. `insertTextMode: 2` is accepted for a single-line primary insertion, where indentation adjustment has no effect; multiline primary indentation adjustment remains unsupported. Additional import edits preserve their exact text.

No completion command is executed. The exact JDT LS callback `java.completion.onDidSelect` is deliberately skipped while its validated text edits are applied. Its selection-ranking feedback and automatic signature-help follow-up are unavailable. The official [JDT handler](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/main/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/CompletionHandler.java) handles that callback separately from text/import edits.

### Problems and navigation

Versioned diagnostics are marked current only when they match the synchronized draft. Older results are dimmed or ignored. Results without a version are explicitly labeled **unversioned**, including when they happen to correspond to the latest file. A missing batch version is never treated as proof of freshness. Empty batches clear the file's problems. Lost/oversized event batches produce an incomplete-results warning.

Definitions support ordinary Location and LocationLink responses. Every target file URI is resolved by the workspace agent and checked against the remote workspace root before opening. Outside-root files, dependency archives, `jdt:` targets, and external URLs are unavailable; they are never opened in a browser. Existing dirty tabs remain intact. Hover text and protocol details are inert, copyable text.

## Recent workflows and their boundaries

With execution trust off, use **Tests** to explicitly **Load report**, **Refresh report**
or **Clear report** for one relative XML path. Results describe the bytes read at the
shown revision; they do not run tests or validate unsaved drafts. See the
[report format and limits](../../docs/TEST_RESULTS.md).

**Compare with disk** offers separate clean reload and conservative draft-merge
flows. **Preview merge** is inert; **Apply** rereads disk and updates only the draft
and its separate disk baseline. The draft remains unsaved, with one Undo/Redo.
Ambiguous, touching or overlapping regions are refused. See
[draft merge](../../docs/DRAFT_MERGE.md). Connection loss preserves drafts; reconnect
and uncertain-save reconciliation remain explicit.

Dedicated Windows Java setup, supported JDT identity and stop outcomes are described
in [Java setup](../../docs/WINDOWS_JAVA_SETUP.md). The optional
[Maven leaf mode](../../docs/MAVEN_PROJECTS.md) requires a supported root POM, an existing
local cache and an ASCII data/control path. It does not download missing dependencies
or run builds. Offline Maven resolution is not network isolation or a code sandbox.
**Find Java type** requires an already running trusted **Java / JDT LS** session and an advertised
provider; selection uses confined ordinary file reads and preserves existing dirty
buffers. See [type search](../../docs/JAVA_TYPE_SEARCH.md). Neither model nor type
queries start a server automatically. Hover a disabled Java type/model button for
its reason; unavailable platform controls may be hidden entirely.

## Chinese and CJK display

Cedar's small bundled fonts do not cover Chinese. When CJK text first appears in an editor, file name, or workspace path, Cedar looks for an already installed system font in the background. It prefers a Simplified Chinese face identified from the collection's metadata, including Noto Sans Mono CJK SC on Linux, Microsoft YaHei on Windows, and suitable PingFang/Heiti faces on macOS. Latin code keeps the existing monospace font.

Only known system/user-font locations are checked. One regular font file, up to 32 MiB, is read and validated before being given to egui. There is no font download, installer, directory-wide scan, or redistribution of proprietary system fonts. Missing or oversized fonts produce a visible warning; text stays intact. Rare glyph coverage still depends on the selected font. The first fallback load increases memory use and may cause a short font-atlas rebuild.

The installed Linux Noto collection was verified through actual egui glyph queries: the default fonts did not cover the Chinese test string, while collection face 7 (Noto Sans Mono CJK SC, 19,484,784 bytes) did. Windows/macOS font lookup paths require native-platform validation and are not claimed tested.

```sh
cargo test -p cedar-app real_system_cjk -- --ignored --nocapture
```

## Limits and resource policy

This remains an independent prototype, not a complete replacement for a mature IDE. There is no debugger, interactive PTY, general project configuration UI, semantic token coloring, snippet engine, multi-file refactoring, general persistent settings, or automatic saves to workspace files. Opt-in private recovery retains bounded unsaved text on the frontend computer; review recovery status before relying on it after a crash or force-quit.

- The backend opens/saves files up to 1 MiB; the frontend allows 32 tabs
- Pasted text is retained, never silently truncated. An oversized draft must be shortened or copied before saving or using language features
- Undo retains at most 16 full-text snapshots per tab, initialized once. Completion creates one transaction; tab close/workspace replacement releases its stored history. Large tabs can still use significant memory; this is not a fixed-memory guarantee or a delta-based undo engine
- In-file find retains 10,000 matches; project search requests 500
- Syntax coloring falls back to plain text above 256 KiB
- Completion retains up to 256 candidates/4 MiB, applies at most 128 edits, and enforces a 1 MiB resulting document
- Diagnostic display is bounded to 2,000 entries, 128 files, and 512 KiB of text
- Protocol detail serialization stops at 128 KiB rather than building an unlimited pretty-printed string

Git, commands, and language servers require explicit workspace trust and the agent’s advertised support. The Windows isolated agent supports bounded asynchronous tasks, explicit Git views and dedicated Java/Maven routes. Generic Windows LSP, legacy synchronous Run/GitStatus and DAP remain unavailable. A frontend follows the connected agent’s capabilities rather than inferring support from its own OS or the transport. Local file editing and existing test-report reads do not require command trust.

Normal quitting waits for active tools and explicitly stops the language server. If a draft changes while shutdown is pending, Cedar asks again before discarding it.

## Verification

```sh
cargo test -p cedar-app
cargo clippy -p cedar-app --all-targets -- -D warnings
```

The ordinary suite covers race handling, dirty-close protection, revision-safe saves, debounce/coalescing, stale snapshots, UTF-16/Unicode/CRLF positions, malformed and overlapping edits, lazy imports, cancelled acceptance, diagnostic freshness, bounded history, capability negotiation and legacy compatibility, rejected-save non-mutation, stale handshake isolation, and headless layout at minimum/default window sizes.

An opt-in real Java integration test creates a temporary Eclipse Java project and exercises actual JDT LS through the local workspace protocol and the frontend transaction/egui undo code:

```sh
CEDAR_JDTLS_HOME=/path/to/jdtls \
  cargo test -p cedar-app real_java_completion -- --ignored --nocapture
```

It checks semantic diagnostics, definition/URI resolution, GregorianCalendar completion, lazy import resolution, atomic application, one-step undo/redo, unchanged disk contents, subsequent synchronization, and shutdown. Evidence is in `tests/evidence/jdtls-1.61.0-editor.json`. That test does not claim OS-window keyboard coverage or real Kotlin/SSH interoperability. Native-window tests and additional server/environment tests must be reported separately.

Framework reference: <https://docs.rs/eframe/0.31.1/eframe/>.
