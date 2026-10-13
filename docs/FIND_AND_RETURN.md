# Find and return

Find and return brings filename discovery, project text results and an explicit
read-only preview into one window. You can inspect a file, open it in the editor
and return to the selection you left, while keeping unsaved drafts intact.

The latest accepted bounded baseline is **0.63.3**, including the connected
browsing shell, retained grouped Project Text results, shared Directory and File
masks, and same-row click continuity before double-click Open. Its exact CI and
package results are separate from the bounded Linux native observations in the
[verification report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT.md#accepted-0633-bounded-browsing-shell-baseline).

## 0.64 Recent Files candidate

[Recent Files](RECENT_FILES.md) is a separate, not-yet-accepted path list for the
current connection. Ctrl+E on Windows/Linux or Cmd+E on macOS opens its local
filter and Open/Closed rows; macOS Ctrl+E retains its editor behavior. It remembers
only settled active paths with known revisions, within 64 paths, 256 KiB total
path bytes and 4,096 bytes per path. It does not discover files or preview them.

Opening a live dirty file reuses its buffer without a Read. A closed path uses
ordinary Open/Read with its default caret position. Recent Files stores and
restores no closed text, caret or selection. Clear, disconnect, reconnect,
workspace change and restart clear the list; idle frames cannot repopulate it.
Final-version aggregate, Python, strict Clippy, formatting and current normal-agent
process proofs have passed, including the Recent/ordinary-Open controls. Exact
CI, package and native acceptance remain pending in the [0.64 report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE64_RECENT_FILES.md).

## Accepted 0.63.3 connected shell

The retained 0.63.1 correction lets Escape dismiss the currently owned Workspace or Tools
menu at the existing generic dismissal branch. Earlier modal and language Escape
owners retain priority; an owned-menu dismissal does not also dismiss editor
Find or mutate a draft. Missing or stale menu ownership cannot close another root.

The retained 0.63.2 correction permits IME Enabled/Disabled lifecycle notifications
alongside an otherwise clean owned Find Results pointer gesture. Active composition,
Preedit/Commit (even empty), typing, paste and competing input still veto activation.
This pointer alignment does not establish the cause of prior native input
observations. Keyboard button activation retains its stricter original-event
contract.

The narrow 0.63.3 shortcut correction handles a clean Alt+Num1 followed by exactly
one `Text("1")` companion. It consumes that companion at validated admission so
it does not replace an editor selection. Only successful guarded Project focus
arms the same-held-key repeat fence. Other text, multiple Text events, Paste,
IME, AltGr and competing input retain their existing rejection rules. The
key-only shortcut, Space and Ctrl/Cmd+J are unchanged. Local focused, aggregate,
normal-agent, exact CI and package checks passed, followed by the bounded Linux
native journey. The [0.63.3 report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE63_BROWSING_SHELL.md)
keeps those evidence types and their limitations distinct.

The shell puts **Project**, the editor and **Find** in the main connected
workspace. **Workspace** groups connection and setup actions, while connection,
error, recovery and unsaved-draft states remain visible and reachable. **Tools**
keeps access to Git, Commands, Language and Tests. Existing execution and trust
requirements still apply. **Back** and **Forward** move into the editor tab band;
their location-history meaning is unchanged.

The header's **Files** and **Project Text** buttons open their existing
search providers; **Find Results** returns to the retained bottom panel.

The **Alt+1** route focuses a useful enabled Project control without
hiding the retained Find tool. **Ctrl/Cmd+J** hides the tools and restores the
captured editor context; reopening Find targets an active visible selected hit,
or its query field when no such hit can be used. Neither shortcut submits a
search, opens a file or refreshes the tree by itself. Pending, inactive, hidden
and disconnected contexts have their own guard checks. The fresh bounded native
journey established the Tree focus route; broader platform and layout claims do
not follow from that observation.

Compact header sizes are design targets, not a fixed-height guarantee. At
**780 × 540**, with both **13** and **18** font sizes, controls must remain
reachable through wrapping or scrolling. Frame checks do not establish arbitrary
native geometry, and Linux native keyboard evidence does not establish Windows
or macOS GUI behavior. Earlier presentation differences are explicitly versioned
below.

## Start with a connected workspace

Select **Find…** in the header, or use **Ctrl+Shift+N** for **Files** and
**Ctrl+Shift+F** for **Project Text**. The window shows the workspace being searched.
Use its provider buttons to switch between filenames and text. The steps below
describe this shared window; the retained bottom tool is described separately
under [Retained Project Text results](#retained-project-text-results).

1. Enter a query, then press **Enter** in the query field or select **Find**.
2. Press **Down** to focus the first result. Use **Up/Down** or **Home/End** to move
   through results, or single-click a row to select it.
3. With a result focused, press **Space**, or select **Preview**, to inspect it.
   Selection and hover alone do not request a preview.
4. Press **Escape** to leave the preview and return to the selected row. Press
   **Enter** on a result, double-click it, or select **Open** to open that file.
5. Use the editor's **Back** button or **Ctrl+[** to return. **Forward** or
   **Ctrl+]** goes forward again. On macOS, the existing history shortcuts use Cmd.

**Ctrl+Alt+Left/Right** are additional Back/Forward shortcuts. **Alt+1**, like the
retained **Ctrl/Cmd+Shift+E**, hides tools and focuses the enabled Explorer
**Refresh** control in the accepted 0.62.2 shell. The accepted 0.63.3 Project route
above changes that presentation and preserves Find. It does not refresh, scan or read a file by itself. These
shortcuts retain the existing modal, mixed-input and held-key guards.
In the accepted 0.61 Linux native run, Ctrl+[ and Ctrl+] navigated successfully,
but Ctrl+Alt+Left/Right did not produce observed navigation when history was
available. The cause remains unknown; those aliases retain this explicit
native limitation.

In this shared window, Tab and Shift+Tab move between controls. Escape when no
preview is open, or
**Close**, hides Find and return and restores the captured editor focus while
that editor context remains current. Hiding the window keeps an accepted Project
Text snapshot for this session. It retires pending preview and activation intent;
late replies cannot reopen a hidden preview or activate a different result.

Query or Project Text filter edits make previous text results inactive. Project
files scope edits also retire that discovery result. Submit explicitly to
activate a new set; changing a field back to its earlier text does not
reactivate the old rows. Switching provider retires pending intent without
discarding the retained Project Text snapshot.
Reconnect requires a new explicit successful text search before historical rows
can be used again.

## Files: check the provider label

**Project files** is available only when the connected agent advertises the
supported filename-discovery extension, version 1. A submitted nonempty query
searches directories that have never been expanded in Explorer, within the
selected scope and the limits below. It reads directory metadata, not file contents, and does
not create a persistent index or watch the filesystem.

Leave **Directory scope** empty to search the workspace. To search one subtree,
enter its exact workspace-relative directory, such as `src/main`. Use `/`
separators; absolute paths, backslashes, empty components, `.` and `..` are
rejected. A nonempty scope still requires a filename query. This control belongs
to Project files; Project Text has its own shared **Directory** and **File
masks** controls described below.

An empty filename query shows open buffers and already loaded Explorer files,
without starting discovery. If discovery is unavailable, the label explicitly
reads **Open and loaded files**. Searches then filter that bounded snapshot only;
they cannot find files in untouched directories, and directory scope is
unavailable. At most 64 matching rows are shown. Reopen Files after loading more
Explorer directories to refresh the snapshot. **Ctrl/Cmd+P** retains its existing
current-directory and loaded-file chooser.

An older agent, or one advertising an unsupported discovery version, keeps this
fallback and receives no discovery requests. Cedar uses the negotiated extension,
not the agent's product version or a trial request. Protocol 4 and existing
capability/trust meanings remain unchanged. Malformed handshake metadata is
rejected; reconnect after replacing an agent to negotiate its support again.

## Matching, exclusions and incomplete scans

Project files accepts a nonempty single-line query of at most **256 UTF-8 bytes**,
without control characters. Matching compares the Rust Unicode lowercase form of
the query and path. It does not perform Unicode normalization or full case folding,
regular expressions, typo correction or CamelCase matching. Results retain their
exact original path spelling. Searches require explicit submission; there is no
live search-as-you-type mode.

Results rank by exact basename, basename prefix, basename substring, then path
substring. Equal ranks sort by original path. Ordering covers the candidates
actually observed; an incomplete traversal cannot promise the best matches from
the entire project.

The fixed excluded directory names are `.git`, `target`, `build`, `dist`,
`node_modules`, `.idea`, `.venv`, `venv` and `__pycache__`, matched exactly. A scope
inside one is rejected. Discovery skips symbolic links, Windows reparse points and
special files. It does not interpret `.gitignore` or IDE module exclusions.

Each submitted scan is bounded by:

- 100 returned paths in this interface; the operation supports at most 200
- 20,000 examined entries and depth 64 below the chosen scope
- A cooperative two-second elapsed budget
- 4,096 pending directories and 512 KiB of pending path text
- 4,096 UTF-8 bytes per relative path and 512 KiB of encoded result data

A blocking filesystem call can outlast the elapsed budget. Only one discovery
request runs at a time, with at most one newer submission waiting to replace it.
Typing alone starts no scan or content Read, and retiring a query does not
disconnect the transport or forcibly interrupt a filesystem call.

**Incomplete scan** identifies permission or I/O errors, unsafe or unrepresentable
paths, or depth, time, entry, pending-directory, response or result limits. Try a
narrower directory scope for traversal limits; changing query text alone may not
reduce traversal. No matches in an incomplete scan does not mean the file is
absent. Even a complete no-match result applies only to that scope and exclusion
policy. A discovered binary or large file can still be refused by Open or Preview.

## Retained Project Text results

**Project Text** explicitly submits a case-sensitive literal search of disk text.
It does not search unsaved draft bytes. Results display the submitted query and
filters with historical line hints; those coordinates may no longer identify
the same text.
The existing engine is bounded, and the interface retains one accepted snapshot
of at most **500 results** and **512 KiB** of combined path/snippet text. The
shared Find and return window and the **FIND RESULTS** tool show that same
snapshot; they do not keep independent search histories or duplicate result text.
A limit warning can reflect traversal, time or response limits, not just the
number of matches.

Select **Search** in the sidebar to open the retained **FIND RESULTS** tool.
Enter a query and select **Search**, or press **Enter** in its query field. The
shared **Project Text** entry continues to use **Find**. Both are explicit
submissions. While a search request is outstanding, another text submission is
disabled, including after an edit makes its original intent inactive. Typing,
hiding, reopening and switching views do not start a search.

### Directory and file masks (0.62.2 bounded acceptance)

Both text-search views edit the same **Directory** and **File masks** fields.
Leave Directory empty for the workspace root, or enter an exact relative
directory such as `src/main`. It is not trimmed or normalized. Absolute paths,
backslashes, colons, control characters, empty components, `.` and `..` are
rejected; the path is limited to 4,096 UTF-8 bytes. A scope inside an excluded
directory is invalid.

Leave File masks empty for all eligible files, or enter semicolon-separated
alternatives such as `*.rs; *.toml`. Whitespace around each pattern is trimmed;
empty patterns are rejected. Use at most eight patterns totaling 256 UTF-8
bytes. Masks match the whole basename, case-sensitively: `*` matches zero or
more Unicode scalar values, and `?` exactly one. There is no regex, gitignore,
character-class or path-pattern mode. The precise syntax and exclusions are
in [Scoped Project Text search](SCOPED_PROJECT_TEXT.md#submit-a-directory-and-filename-mask).

With both filters empty, submission uses unchanged legacy Search. Any filter
requires the connected agent's supported scoped-search version 1 descriptor.
An older or future-version peer receives zero scoped requests; an explanation
asks you to clear both filters or use a compatible agent. Cedar does not
silently run a broad workspace search instead.

Filtered search starts backend traversal at Directory and tests masks before
opening content, including in folders never loaded in Explorer. Its independent
bounds are 20,000 entries, depth 64, a cooperative five-second budget, 4,096
pending directories with 512 KiB of path text, 1 MiB per file, 32 MiB actually
read, 500 requested interface rows and 512 KiB of compact encoded
`ScopedSearchResult` data, excluding enclosing response framing. The backend
supports at most 1,000 rows. Blocking filesystem calls can exceed the elapsed
budget.

The fixed directory exclusions are `.git`, `target`, `build`, `dist`,
`node_modules`, `.idea`, `.venv`, `venv` and `__pycache__`. Links, reparse points,
special files, binary or invalid-UTF-8 text and oversized files are excluded.
The view shows examined entries, bytes read and distinct incomplete reasons.
**No matches in an incomplete scan does not establish absence**; even a complete
no-match result applies only to the submitted scope, masks and exclusions. A
narrower Directory may reduce traversal; narrower masks alone may not. See the
[backend bounds and incomplete contract](SCOPED_PROJECT_TEXT.md#backend-work-and-limits).

### Group, inspect and return

Results are grouped by their exact original path. Groups follow the first
appearance of each path in the accepted response, and each group's hits retain
their original relative order. Equal-looking hits remain separate rows. Grouping
does not rewrite paths or change which original hit an action addresses.

- Single-click a hit to select it. Selection and hover do not read file contents.
- With a hit focused, **Up/Down** and **Home/End** navigate visible hits.
  **Space** requests a read-only preview; **Enter** or a double-click uses
  ordinary Open. A double-click must follow an accepted click on the same exact
  hit in the same view and preview pane. A rapid group click followed by the
  first click on another hit selects it; it does not qualify that hit for Open.
  Switching views, focus or result identity breaks the click sequence.
- Click a path header to expand or collapse its group, or reach it with **Tab**
  and press **Enter** or **Space**. Collapsing a group preserves its selected hit
  and puts focus on the group header. A hidden hit cannot be previewed or opened;
  expand its group to act on it again.

In the bottom FIND RESULTS tool, preview leaves focus on the selected row.
Use the existing tool close/hide control or **Ctrl/Cmd+J** to return to the editor;
**Ctrl/Cmd+J** also reopens the selected tool. The accepted 0.63.3 shell uses the
active-visible-hit or query focus route described above. The bottom tool does
not add an Escape action. The shared Find and return window keeps its preview/dismiss
Escape behavior described above.

The submitted query and filters, selected hit and group expansion state survive
hiding and reopening, switching between the two text-search views, and Open
followed by Back, within the same session. Each view retains its scroll position within the
current layout's scroll limits; resizing can clamp it. Preview intent is separate:
request it again after reopening. A successful rerun replaces the accepted
snapshot; no history is stored across application restarts.

Editing the query or either filter, starting a rerun, a failed rerun or
reconnecting leaves old rows visible as **inactive** historical results. They
cannot preview or Open. Neither restoring the old field values nor reopening
the view renews their
authority. Run **Search** in FIND RESULTS, or **Find** in Project Text, and wait
for a successful result for the current query, filters and connection. The
accepted snapshot retains the complete submitted tuple and extension version
or legacy mode; newer field edits cannot change what its historical results
describe.

Both Find views use compact local spacing and place results and preview side by
side when there is enough width. In narrower views, preview stacks below the
results and their controls. A very short manually chosen tools height can clip
or hide result rows, especially with larger text. Fully invisible rows cannot
preview or Open, even if a row previously had keyboard focus. Select
**Automatic height** in the tools header to recover room for the Find journey;
you can also reach that control with **Tab** and activate it with **Enter**.
Then focus a visible hit before previewing or opening it. The stored height is a
preference, not a guarantee that all Find content fits.

These local changes do not change the application's global theme or promise
usability at every tiny viewport. Case/word options, regular expressions,
debounced or live queries, CamelCase matching and disk-persistent search history
are not part of the 0.62 scoped-search addition.

## Explicit read-only preview

Preview is separate from an editable tab and is limited to **1 MiB**. For a file
already open, **Current open draft** shows that exact draft and sends **zero
backend Reads**. For an unopened file, an explicit preview uses a bounded Read.
Unreadable, binary, oversized or invalid responses show an explanation. There is
one retained preview and at most one preview Read in flight across both Find
views, with one replacement selection waiting. Text hits carry no file revision,
so a disk preview cannot establish that the file is unchanged since the search.

After cancelling or hiding a preview, explicitly previewing the same hit again
starts a fresh preview intent. A reply belonging to the cancelled preview cannot
supply the new one. The same single-flight and size limits still apply.

**Open** always uses ordinary file opening; preview bytes are not promoted into
a new editable document. An existing dirty tab is reused without rereading or
replacing its text or Undo history. Filename opening preserves that tab's
selection; a text hit deliberately navigates to its historical line hint. No
find, selection, preview, Open or Back action implicitly saves a file.

Back restores the departure selection, including its direction, only while the
original tab is still open and its edit version is unchanged. Edited or closed
locations are skipped; Undo to identical text does not revive an old location.
History is bounded and local to the session, not a persistent recent-files list.
Reconnect or workspace changes clear it.

## Existing tools and remote access

Filename discovery, text search and preview use Cedar's existing workspace agent
and path checks. They do not enable execution trust, start Git, commands or a
language server, or add a new remote transport. Existing local and configured SSH
connections keep their current setup and authentication boundary. This candidate
does not establish new real-SSH acceptance or a sandbox against hostile concurrent
filesystem changes.

Build, Run and other existing tools remain available under their existing support
and trust requirements. Java type search remains in the Language tools and needs
an already running supported, trusted Java session. This browsing flow does not
start it automatically or provide every-language symbol search. It makes no IDEA
parity or unmeasured performance claim.
