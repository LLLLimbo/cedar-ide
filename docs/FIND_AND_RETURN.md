# Find and return

Find and return brings filename discovery, project text results and an explicit
read-only preview into one window. You can inspect a file, open it in the editor
and return to the selection you left, while keeping unsaved drafts intact.

The current **0.66 continuous Project Text preview candidate is not yet
accepted**. Final local focused/aggregate checks, strict Clippy, formatting and
current-artifact normal-agent proofs passed. Fresh exact CI, packages and native
validation have not run. The candidate contract is described
[below](#continuous-project-text-preview-066-candidate); Files keeps its existing
explicit-preview interaction.

The latest accepted bounded baseline is **0.65.1**, with exact CI, three package
audits and a fresh bounded Linux native journey passed. That journey covered a
readable `src/层/Target.rs` path, Tree/Flat current-file selection, Down/Up,
dirty-selection and Undo/Redo preservation, and retained Find state. Native
cancellation, Windows/macOS GUI, real SSH, arbitrary viewports and broad CJK
coverage remain unverified. The 0.65.0 glyph stop and 0.64.0 strict timing failure
remain part of the
[verification report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT.md#accepted-0651-bounded-baseline).

## Recent Files (0.64 bounded functional acceptance)

[Recent Files](RECENT_FILES.md) is a separate path list for the
current connection. Ctrl+E on Windows/Linux or Cmd+E on macOS opens its local
filter and Open/Closed rows; macOS Ctrl+E retains its editor behavior. It remembers
only settled active paths with known revisions, within 64 paths, 256 KiB total
path bytes and 4,096 bytes per path. It does not discover files or preview them.

Opening a live dirty file reuses its buffer without a Read. A closed path uses
ordinary Open/Read with its default caret position. Recent Files stores and
restores no closed text, caret or selection. Clear, disconnect, reconnect,
workspace change and restart clear the list; idle frames cannot repopulate it.
Its checks and the preserved strict native timing failure are recorded separately
in the [0.64 report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE64_RECENT_FILES.md).

<a id="select-current-file-in-project-065-candidate"></a>

## Select Current File in Project (0.65.1 bounded acceptance)

**0.65.1 passed its bounded acceptance.** The exact CI, three package audits
and fresh Linux native checks cover the current-file selection journey described
above. Cancellation is checked by deterministic and normal-agent proofs, but
was not exercised in that native journey. The preceding 0.65.0 native run stopped
at a Project Text CJK path display problem; its skipped core checks and failure
remain recorded rather than being reclassified as passed.
The explicit, visible
**Select Current File in Project** button below the Flat/Tree controls selects
the active file in Project without opening or rereading it. The chosen **Flat**
or **Tree** mode stays in place. Flat sends at most **one List of the known parent
directory**, when needed; Tree uses only the known ancestor chain needed to
expose that exact path.
Existing accepted cache entries can be reused. It does not search for the file
in other directories or run a recursive or project-wide scan; only the exact
known parent or ancestor directories are listed.

The action shares Explorer's single in-flight List slot. If Project is already
waiting for a List, wait and select the current file again; no attempt is queued.
One Tree attempt admits at most **32 List requests**. Both modes use a **fixed
30-second total deadline** that is not renewed by replies. Directory replies
still obey the existing path, entry and
cache bounds. A missing, invalid, stale or over-budget reply cannot be presented
as a successful selection. Success requires the exact file row to be selected,
focused and visible in the current Project view, after a final identity check.

**Cancel selecting current file** remains visible and is disabled while idle,
keeping the controls in place. While selection is pending, it retires the intent.
Cancellation is conservative: new meaningful input, window blur,
editor document/path/edit-version changes,
navigation, mode or connection changes, and manual Project commands also retire
the old intent. Pointer movement and the initiating key's matching release alone
do not. For an intent already admitted from clean input, IME Enabled/Disabled
lifecycle notifications alone also do not cancel it. Active composition still
blocks entry; Preedit/Commit events, even empty, and mixed meaningful input
cancel pending intent. The existing keyboard-entry rules are unchanged.
Late replies cannot focus another row or continue an old ancestor
sequence. The existing per-List timeout remains 30 seconds; the selection
deadline expires independently, including before the final row is rendered.
An already-admitted ordinary List may settle through its existing guards after
cancellation or expiry, but cannot continue or focus the old selection intent.
In Flat mode, that List can still replace the displayed parent directory;
cancellation does not roll back admitted directory work or forcibly interrupt
an already-running filesystem call. A new explicit attempt is needed
after a failed or cancelled attempt. Switching editor tabs or repainting alone
never requests automatic file selection.

Selection keeps the active editor's dirty text, selection, Undo and location
history intact, and retains Find state. It sends no content Read, discovery or
search request and adds no backend operation or protocol extension. **Alt+1**
keeps its existing zero-request Project-focus behavior; it does not invoke this
action. The [0.65 report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE65_PROJECT_REVEAL.md)
separates the accepted 0.65.1 evidence from earlier failures and platform limits.

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
   In the 0.66 candidate, this also arms continuous preview for that exact Project
   Text view and result snapshot; subsequent owned selections settle for 150 ms.
   Unarmed selection and hover never request a preview. Files remains explicit.
4. Press **Escape** to leave the preview; for Text it also disarms continuous
   preview and leaves focus on the selected row. Press **Enter** on a result,
   double-click it, or select **Open** to open that file.
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

**0.65.1 bounded CJK acceptance:** current, validated, accepted result paths and
snippets can trigger the existing background local font loader.
It checks known font locations and reads one validated regular font file within
32 MiB; it does not download or install fonts. Display still depends on the
installed font's glyph coverage. The fresh bounded Linux native journey confirmed
`src/层/Target.rs` was readable; it does not establish general CJK or IME coverage.

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

- Single-click a hit to select it. Unarmed selection and hover do not read file
  contents. In the 0.66 candidate, an armed Text view can update its preview after
  an owned selection settles, as described below.
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

In the 0.66 candidate, preview leaves focus on the selected row in both Text
views. In the bottom FIND RESULTS tool, use the existing tool close/hide control or **Ctrl/Cmd+J** to return to the editor;
**Ctrl/Cmd+J** also reopens the selected tool. The accepted 0.63.3 shell uses the
active-visible-hit or query focus route described above. For an armed Text
preview, **Escape** dismisses the preview and disarms it while retaining
result-row focus. The shared Find and return window keeps its later
Escape-to-dismiss behavior when no preview is active.

The submitted query and filters, selected hit and group expansion state survive
hiding and reopening, switching between the two text-search views, and Open
followed by Back, within the same session. Each view retains its scroll position within the
current layout's scroll limits; resizing can clamp it. Preview intent and
continuous-preview arming are separate from retained results: request preview
explicitly again after reopening. A successful rerun replaces the accepted
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

## Continuous Project Text preview (0.66 candidate)

This candidate applies only to **Project Text in Find and return** and the
bottom **FIND RESULTS** tool. It leaves Files preview and search submission
unchanged. **Space** on an owned current hit or the labelled **Preview** action
arms only the exact current Text surface and accepted result view. The first
preview is immediate and keeps result-row focus. Selecting or hovering over a
result before that explicit action sends **zero preview Reads**; typing a query
also sends none.

Once armed, a real, owned selection change through a click or result-navigation
key selects the next candidate. It must remain selected for **150 ms** before
preview admission. This is a fixed settlement interval for that selection;
repainting, hover or passive focus alone cannot start or renew it. Moving to a
new candidate replaces the previous candidate. There is at most **one preview
Read in flight** and **one latest replacement candidate** across the two views,
not a queue of intermediate rows. A late success or error cannot display an
older candidate or steal focus. Retiring intent does not forcibly cancel an
already-running filesystem call.

Text previews show read-only line context around the historical hit: at most
**41 lines** and **64 KiB of UTF-8 text**, with truncation identified and clipping
only at valid UTF-8 boundaries. The indicated line is labelled as a **historical
line hint**, not a verified current match. The source may have changed since
search. A historical line beyond the current source's EOF shows **unavailable**;
it is not silently clamped to the last line. An open source uses its exact current
draft with **zero Read**, including an unsaved draft. Preview never moves that
editor's selection or changes its text or Undo history.

**Escape**, hiding or dismissing the view, query/filter edits, rerun, provider or
surface changes, bound document-version changes, navigation and reconnect disarm
continuous preview and retire its pending intent. Opening a result uses ordinary
Open and retires preview intent. Reopening retained results does not rearm or
automatically Read; use Space or Preview again. Late work cannot reopen a hidden
preview or restore old focus. Switching to the other Text view requires a new
explicit preview even though both views share the retained result snapshot.

The candidate adds no protocol, backend operation, service, index, editable
preview tab, cross-session persistence or Save action. Its validation status is
recorded in the
[0.66 report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE66_CONTINUOUS_PREVIEW.md).

## Read-only preview and ordinary Open

Preview is separate from an editable tab. Source files retain the existing
**1 MiB** Read limit; the 0.66 Text context display has the smaller bounds above.
**Files** keeps its existing explicit-preview behavior and 1 MiB preview limit.
For a file already open, **Current open draft** shows that exact draft and sends
**zero backend Reads**. An unopened file uses the existing bounded Read.
Unreadable, binary, oversized or invalid responses show an explanation. One
retained preview and the shared single-flight Read limit remain. Text hits carry
no file revision, so a disk preview cannot establish unchanged bytes since search.

After cancelling or hiding a preview, explicitly previewing the same hit again
starts a fresh intent. A reply belonging to the cancelled preview cannot supply
the new one. For Text, that fresh action also rearms only the current exact view.

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
