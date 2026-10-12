# Cedar native desktop frontend

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../../PUBLICATION.md#verification-evidence).

Cedar is a native Rust application using `eframe`/`egui` 0.31.1 and the Glow renderer. It does not embed a browser or a JVM. The remote workspace agent and optional language server run separately from the UI.

## Find and return candidate (0.60.0)

Use **Find…**, **Ctrl+Shift+N** for Files, or **Ctrl+Shift+F** for Project Text.
Submit a query, select a result, request **Preview**, then use ordinary **Open**
and **Back**. Negotiated Project files discovery includes untouched directories;
the labelled fallback searches only open and loaded files. Draft preview sends no
Read and browsing does not implicitly save. See the [usage and limits](../../docs/FIND_AND_RETURN.md).
Filename queries use explicit submission and literal Unicode lowercase matching;
regex, CamelCase matching and search-as-you-type are not implemented.
This candidate still needs full CI, package checks and native-window acceptance;
it does not change the accepted status of earlier checkpoints or claim visual
parity or a performance advantage. Existing Build/Run features remain available.

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
- Explicit project text search with historical line results; 0.57.6 passed complete dual-platform CI, all three packages and bounded Linux native acceptance, with an exact inactive-row pointer-hit limitation retained
- Trusted-workspace Git status and explicit executable commands with literal argument rows, cancellable task output, and saved project profiles in `cedar.tasks.json`

Shortcuts:

| Shortcut | Action |
| --- | --- |
| Ctrl/Cmd+P | Choose an open buffer or current-directory file; explicitly open a relative path |
| Ctrl+Shift+N | Open Find and return with Files selected |
| Ctrl+Shift+F | Open Find and return with Project Text selected |
| Ctrl/Cmd+[ or Ctrl+Alt+Left | Back to an unchanged open-buffer location |
| Ctrl/Cmd+] or Ctrl+Alt+Right | Forward to an unchanged open-buffer location |
| Ctrl/Cmd+G | Go to a 1-based line in the current buffer |
| Ctrl/Cmd+F | Find and preview literal replacements in the current buffer |
| Ctrl/Cmd+S | Save |
| Ctrl/Cmd+W | Close the current tab |
| Ctrl/Cmd+J | Hide tools and return to the editor, or reopen the selected tool |
| Ctrl/Cmd+Shift+E or Alt+1 | Hide tools and focus enabled Explorer Refresh without refreshing |
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

The accepted 0.56.1 checkpoint retains an explicit tools-panel height only after an actual
primary-button vertical drag on its native top divider. Until then, content can
still shrink the panel automatically. The explicit choice lasts through empty,
loading, loaded and cleared reports, hide/show, tool changes and reconnects in
this frontend process. Later drags can enlarge or reduce it. Window constraints
may temporarily limit the displayed height without replacing the preference;
window growth restores it when space permits. Restarting the app returns to
automatic sizing. The existing 245-point default and 150–500 range remain, but
neither a hard content cap nor room for the editor is guaranteed. Dual-platform
CI and all three packages passed; bounded Linux native acceptance combines the
original partial run with a separate same-package shrink/regrow check. Original
misses and limits remain in the [0.56.1 report](../../docs/TEST_REPORT_PHASE561_TOOLS_HEIGHT.md#final-accepted-0561-evidence).

The **Automatic height** control added in 0.58.0 explicitly clears the runtime
height preference after the current panel pass and requests a repaint using the
existing automatic content-sizing behavior. A later genuine divider drag can
establish an explicit height again. It does not persist settings or send a
workspace request. **0.58.4 passed this bounded acceptance**:
complete dual-platform CI, retained process gates and all three packages passed,
including 15 editor-entry, 27 resize and 23 sidebar checks on each platform.
Fresh exact-package cloud Linux GUI evidence covers actual divider drags,
pointer/keyboard reset and native Tab entry followed immediately by Undo/Redo
without pointer refocus. The critical sequence is steps 56/57/58, after 37 real
Tab presses; the anticipatory step 55 filename does not prove editor entry.
No Save, Run, LSP or source-disk change occurred. The 0.58.3 boundary-button fixture
failures and earlier shipping failures remain preserved in the
[current report](../../docs/TEST_REPORT.md#final-accepted-0584-evidence).
No Windows GUI, raw-event ownership, native request-count or native agent-reap
claim follows. The command-output banner remains backlog.

## Project Search keyboard navigation (0.57.6 bounded acceptance)

The retained 0.57.6 Search checkpoint passed complete dual-platform CI: all retained
process gates and all three packages passed. Both platforms passed 33 Search and
19 report-keyboard tests and the exact 14-case/57-field/12-request Search proof.
A fresh exact-package cloud Linux native run observed Space opening line 97 and
an isolated preceding-row click reaching line 96, plus historical-query display,
fresh-query reactivation and reconnect followed by explicit re-search.

The edited-query inactive rows were visibly disabled, and an attempted click
produced no navigation. Their roughly 20-pixel layout shift prevents an exact
inactive-widget hit claim; that guard is established by CI/process proofs. The
native run does not establish raw event batches, backend request counts or
owned-agent reaping. It does not explain the preserved 0.57.1 native failure or
claim Windows GUI acceptance. The [final 0.57.6 evidence](../../docs/TEST_REPORT_PHASE576_SEARCH_MIXED_INPUT.md#final-accepted-0576-evidence)
retains each original failed verdict and the exact-package native limits.

Shipping Search retains the 0.57.5 classification: MouseMoved(_) is ignored
companion metadata, never read or validated for finiteness, and never used as a
position or geometry source, consumed or replayed. Actual Primary-button absolute
positions, matching capture/view/release/current-frame ownership and the row's
actual egui clicked_by(Primary) response remain mandatory. The 0.57.6 correction
changes only a mixed-input test's focus/key-ownership expectation and bounded
diagnostics, with no added frames, forced focus or shipping change. Deterministic
already-arrived-pointer cases do not guarantee arbitrary merged native input.

Enter a case-sensitive literal query and explicitly select **Search**, or press
Enter in its query field. The accepted result batch labels the submitted query.
These are historical on-disk lines, without file revisions or current-draft
freshness: unsaved changes are not search input, and a recorded line may now point
somewhere else. The existing engine is bounded; a limit warning may reflect scan,
time or response limits as well as row count.

Use native Tab to focus a result row. Standalone, unmodified Up/Down moves through
backend order, Home/End reaches the first/last row, and boundaries stop without
wrapping. The destination scrolls into view; duplicate labels retain distinct
row identity. Movement sends no requests and leaves editor text, selection, Undo
and history unchanged. Other controls keep their normal keys.

Enter/Space or clicking an enabled row opens that exact result through ordinary
Open. Existing dirty text and Undo stay intact, but the caret deliberately moves
to the historical line and the departure is recorded for Back. An unopened file
uses the ordinary root-confined Read path. No match-column or current-draft
position guarantee is made.

Old rows remain visible but inactive while a newer request is pending, the query
differs, or the connection changes. Explicitly search again after reconnect.
Query/batch changes, hidden or switched tools, modals and competing input cancel
unfinished row intent; continue with a fresh valid key or pointer action. The frontend keeps at
most 500 rows and 512 KiB of combined path/snippet bytes in one accepted batch;
oversized batches are rejected whole. No automatic search, new engine, protocol,
capability or workspace operation was added by that Search checkpoint. The
accepted 0.59.3 Unicode Parameter info renderer checkpoint has separate evidence in the
[current report](../../docs/TEST_REPORT.md).

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

### Explicit Java Parameter info (0.59.3 accepted)

In an already running trusted **Java / JDT LS** session, **Parameter info** requests
a read-only signature snapshot at the exact synchronized Java draft version and
UTF-16 cursor. Both the agent's optional operation and the actual server provider
must support it. **Overload 1**, **Overload 2** and subsequent controls browse the
returned signatures locally. **Dismiss parameter info** clears the result.

Only a valid server hint earns **Server-selected overload**. With a missing or
invalid signature hint, the UI says **First overload shown; the server did not
select a valid overload.** A usable per-signature parameter hint wins over the
top-level hint; an out-of-range per-signature hint produces no highlight and does
not fall back. Missing or invalid hints do not guess parameter zero. UTF-16
ranges must be valid; string labels highlight only a unique occurrence.

The shared parser caps 32 signatures, 64 parameters per signature, 8 KiB per
retained label and 128 KiB total retained UTF-8 label text. Documentation and
server extension fields are not interpreted. Edit/cursor/tab/navigation changes,
workspace/connection/session generations, newer queries, dismissal and hiding the
view invalidate it. Late responses cannot revive an old view. The action does
not edit source, move the selection, create an Undo record or save. There is no
automatic typing callback or new activation shortcut. Synchronization does not
prove that JDT has finished indexing or analyzing.

Version **0.59.2 passed bounded acceptance**: exact-commit dual-platform CI,
all four jobs, three package audits, both real-JDT three-query proofs and bounded
Linux native functional observations with a separate cleanup complement. The
first native 125.452-second cleanup failure against its 120-second limit remains
failed; the same-package complement completed in 28.175 seconds. Original Java
source was unchanged, while JDT rewrote generated `.project` metadata and added
a class file. No whole-workspace equality or native agent-reaping claim is made.

Version **0.59.3 passed bounded acceptance** at public commit
`1e2a26d6dd38c17ed3723ced54797fcb43255eb6` in
[CI 38192432380](https://github.com/LLLLimbo/cedar-ide/actions/runs/38192432380):
all four jobs and three exact-source packages passed. Linux recorded 1,862 passed
and 67 ignored; Windows recorded 1,805 passed and 111 ignored, with zero failures.
It adds actual-renderer Unicode tests without changing production behavior.
Native provenance is carried from 0.59.2 through verified shipping equivalence;
no new GUI run was performed. Original private native artifacts are unavailable
in the current workspace, so no fresh native recheck is claimed. The
0.59.0/0.59.1 failures remain preserved, and the 0.59.1 temporary-path alias
explanation is still unconfirmed at runtime. See the
[complete contract and LSP default deviation](../../docs/JAVA_PARAMETER_INFO.md)
and [current verification report](../../docs/TEST_REPORT.md).

### Problems and navigation

**Go to Implementations** explicitly synchronizes open Java drafts and requests
the current cursor's implementation locations from a running typed Java session.
It requires the agent and server provider. Result selection uses ordinary
root-confined file navigation and retains dirty buffers and Undo/Redo. These are
unversioned JDT locations, not a call graph; type queries may return subtypes and
method results identify declarations rather than every inheriting class.
Hiding tools or leaving the result view dismisses this captured navigation
snapshot. See [scope and limits](../../docs/JAVA_IMPLEMENTATIONS.md).

Versioned diagnostics are marked current only when they match the synchronized draft. Older results are dimmed or ignored. Results without a version are explicitly labeled **unversioned**, including when they happen to correspond to the latest file. A missing batch version is never treated as proof of freshness. Empty batches clear the file's problems. Lost/oversized event batches produce an incomplete-results warning.

Definitions support ordinary Location and LocationLink responses. Every target file URI is resolved by the workspace agent and checked against the remote workspace root before opening. Outside-root files, dependency archives, `jdt:` targets, and external URLs are unavailable; they are never opened in a browser. Existing dirty tabs remain intact. Hover text and protocol details are inert, copyable text.

## Recent workflows and their boundaries

With execution trust off, use **Tests** to explicitly **Load report**, **Refresh report**
or **Clear report** for one relative XML path. Results describe the bytes read at the
shown revision; they do not run tests or validate unsaved drafts. The bounded
[0.55.4 acceptance](../../docs/TEST_REPORT_PHASE554_QUICK_DIAGNOSTICS.md#final-accepted-0554-evidence)
covers Up/Down/Home/End traversal of filtered rows in original XML order.
Enter/Space reveals the focused row's exact details; **Back to case** returns to
that row. It preserves native Tab/filter controls and keeps duplicate names
separate. See the [report format and limits](../../docs/TEST_RESULTS.md).

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
- In-file find retains 10,000 matches; project search requests and retains at most 500, with 512 KiB of combined path/snippet bytes
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
