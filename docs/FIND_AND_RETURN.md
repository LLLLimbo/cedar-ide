# Find and return

## 0.80 development checkpoint: text matching options

Project Text in the shared Find window and bottom Find Results uses one
**Match case** draft, checked by default. Uncheck it, then explicitly submit for
Unicode 16.0 simple-fold literal matching on a `case_search` v1 agent. It does not
automatically search, normalize text, perform full folding, enable regex or
whole-word matching, or change Directory/mask/filename/Project-name semantics.
The checked legacy routes and unsupported-peer fail-closed behavior are retained.
See [the current scoped/text contract](SCOPED_PROJECT_TEXT.md#current-match-case-control-080-development-checkpoint)
and [checkpoint scope and fresh acceptance criteria](TEST_REPORT_PHASE80_DEVELOPMENT_CHECKPOINT.md).
The historical sections below retain their original version and evidence.

Find and return brings filename discovery, project text results and an explicit
read-only preview into one window. You can inspect a file, open it in the editor
and return to the selection you left, while keeping unsaved drafts intact.

The **0.76.0 Files native IME acceptance candidate** requires an actual native
TextEdit acceptance observation in addition to the existing ownership and
cancellation rules. Its self-contained offline interaction boundary is
[below](#files-native-ime-acceptance-076-candidate). Final-version verification,
exact-source public CI and package acceptance remain separate pending gates;
real OS IME and new native GUI behavior are untested.

The earlier **0.75.0 Files IME focus-handoff candidate** preserves the existing search
contract while preventing cancelled or unaccepted composition from resuming
automatic search across query/Directory focus transfer. The complete offline
interaction boundary is [below](#files-ime-focus-handoff-075-candidate).

The earlier **0.74.0 Recent Files layout candidate** is a separate connection-local
chooser, described in [Recent Files](RECENT_FILES.md). Its filename/parent/status
rows and fixed footer do not add search, preview or a new provider. Recent reopens
with an empty filter and its Undo cleared, unlike the retained Project Text query.
Single/double/triple row clicks only select; Enter/Open performs ordinary Open.
The earlier **0.73.0 Project Text layout candidate** changes the Text presentation described
[below](#project-text-layout-073-candidate). The earlier Files layout and retained
behavioral contracts remain separate from version-specific validation.

The accepted search-as-you-type behavioral baseline is **0.69.0**;
its complete [offline interaction contract](#files-search-as-you-type-069-bounded-acceptance)
is below. **0.70 stale-navigation hardening is a candidate and is not accepted**;
its [separate contract](#stale-navigation-isolation-070-candidate) does not inherit
0.69 verification results.

**0.66.2 passed bounded continuous-preview acceptance.** Exact dual-platform
CI, all three package audits and a fresh bounded Linux native journey passed.
The native journey covered following selection in both Text views, the previously
failed bottom B160 step, ordinary Open/Back, dirty selection and Undo/Redo, and
Escape/hide/reopen. The contract is described
[below](#continuous-project-text-preview-0662-bounded-acceptance); Files keeps
its existing explicit-preview interaction.

The 0.66.0 native bottom-preview failure and 0.66.1 Windows test-caller count
failure, with no 0.66.1 native run, remain recorded. The 0.65.0 glyph stop and
0.64.0 strict timing failure also remain part of the
[verification report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT.md#accepted-0662-bounded-baseline).
Windows/macOS GUI, real SSH, arbitrary viewports and broad CJK/IME coverage
remain unverified. Native pixels do not establish the 150 ms interval, request
counts or raw caret affinities; deterministic and process checks provide their
own evidence.

The separate [0.67.1 File Structure menu](FILE_STRUCTURE.md) passed bounded
acceptance led by exact dual-platform CI and all three package audits. Its
positive symbol paths are covered by frame tests and the Linux normal-agent/mock
proof. Native coverage is limited to the Trust-off visible menu, unavailable
explanation, filter, Escape/reopen and dirty-draft Undo/Redo. Positive native
symbols, real-JDT popup behavior and native Ctrl+F12 remain unmeasured. The
0.67.0 native shortcut-entry failure and independent raw-hook observation are
preserved in the
[phase report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE67_FILE_STRUCTURE.md).

## Files native IME acceptance (0.76 candidate)

Files query and Directory keep their native single-line String editors. A Commit
notification alone cannot prove that the focused editor accepted it: native egui
may reject it after a caret/selection change within that same field. The frontend
now reads payload-free IME branch observations returned by that exact TextEdit
invocation, while retaining the original owner, current batch/epoch, continuous
owner focus, final focus and pause-barrier checks. Missing observations mean the
branch was not observed; they do not mean native rejection or permission to search.
An extra layout pass cannot replay the earlier observation or restart its timer.

For an owned composition, accepted Commit completes the latest native lifecycle;
a later Enabled/Preedit starts another, and empty or standalone CR/LF Commit
cancels completion under the existing conservative rule. A rejected duplicate or
Disabled alone does not erase an already accepted completion in that lifecycle.
An accepted, valid completion starts the full **250 ms** interval. Acceptance
means the actual native insertion branch ran; it does not promise changed bytes
or full insertion. Same-value input and native character-limit truncation retain
their real result, and the existing UTF-8 query/scope validation still applies.

Outside a composition, a genuinely accepted direct Commit remains an independent
edit as in 0.75, including when a later empty/ignored/rejected direct terminal
follows. The existing ownership and pause barrier still apply. Ordinary owned
Text/Paste intent and genuine native byte-changing edits, including Undo, keep
their separate recovery paths. A native IME mutation under a stale epoch or wrong
raw owner cannot bypass the barrier merely because visible bytes changed.

After rejected or cancelled composition, the exact native query and Directory
remain visible and automatic search pauses. Clean fresh edits, a valid fresh
composition, or an actually executed clean Find/Enter can resume it. Pointer or
caret movement, idle, selection-only Undo and old late terminals do not suffice.
A commit already completed before a later focus transfer retains its original
250 ms deadline. Closing or switching the provider clears the local pause state.

The single physical FindFiles flight, latest replacement, stale-reply isolation,
literal matching and validation limits are unchanged. Automatic search does not
Read, preview, Open, activate, save, retry or poll. Project Text remains explicit.
The same-version egui 0.31.1 modification observes native branches; it does not
replace native input handling. Its source provenance and MIT/Apache-2.0 notices
are described in the package's THIRD_PARTY_NOTICES.md, under Vendored egui 0.31.1.

The development evidence uses synthetic events through real egui controls and
production fonts. Real OS input methods, a new GUI journey and Windows/macOS GUI
are unverified. The source-only report is docs/TEST_REPORT_PHASE76_IME_ACCEPTANCE.md;
this packaged interaction guide does not require that report to be present.

## Files IME focus handoff (0.75 candidate)

The following is the historical 0.75 scope. The 0.76 section above adds actual
native acceptance checks for the same-field seam it originally left unverified.

Files keeps the original composing query or Directory control as the owner.
If focus transfers before that control completes its composition, an old terminal
Commit cannot resume automatic search, including after focus returns to the old
field or a separated Disabled event has already ended that lifecycle. The native
final query and Directory bytes remain visible; the app does not replace them
with a guessed commit. A valid commit completed before a later focus transfer
still keeps its original full 250 ms interval and does not restart it.

After an interrupted handoff, fresh native edits can resume automatic search.
In a clean batch containing no IME, a genuine byte change in either Files field,
including Backspace, Undo, Text or Paste, is a new committed edit. In an
IME-containing mixed batch, Text/Paste must carry the original field's owned
intent and actually be handled there. Existing same-value Text and empty-Paste
fresh-intent behavior remains. Selection/caret movement, pointer focus, idle and
a selection-only Undo do not resume it. A new Enabled/Preedit lifecycle belongs
to the then-focused Files field and must complete with continuous owner focus;
an actually executed clean Find/Enter is also an explicit way to search the
visible values. Closing or switching the provider clears this local pause state.

Mixed Text/Paste and valid commits, literal Unicode handling, existing validation
limits, one physical FindFiles flight, latest replacement and stale-reply guards
remain unchanged. Typing still does not automatically Read a file, preview,
activate, save, retry or poll. The development checks use production-font egui
update frames and synthetic raw events; they are not real OS input-method tests.
This is a focus-handoff fix, not a general proof that every native Commit was
accepted. Same-field pointer/caret-only IME interactions are outside this claim.
The source-only phase report is docs/TEST_REPORT_PHASE75_IME_FOCUS_HANDOFF.md;
that report is not required for this self-contained packaged interaction guide.

## Project Text layout (0.73 candidate)

Find in Files is the Project Text provider, with explicit Enter/Find submission
and case-sensitive literal disk matching. Directory and File masks retain the
existing exact scope and semicolon-mask rules. It adds no regex, Words, Replace,
indexing or implicit content reads.

When preview is unarmed or hidden, grouped results occupy the full width.
Basenames are primary, parent paths are muted, and original full Unicode paths
remain the action identities. The summary counts matching lines and exact-path
file groups, not repeated literal occurrences within one line. Previous results
stay visibly inactive after edits until an explicit fresh search succeeds.

Preview/Space explicitly arms read-only context below the results. Selection
following, Hide/Escape, physical Read retirement, dirty drafts and return history
keep their existing behavior. The current status headline is fixed; longer
historical, error and scan details scroll in a bounded area with vertical slack.
Header, status and footer do not change height with the number of results.
Closing and reopening retains query, accepted results and selection; it does not
silently replace them with an empty state or rearm a hidden preview.

Production-frame checks cover normal 1178x814 and minimum 780x540 at 13/18 points;
700x480 is resilience only. The actual normal-window development comparison used
an earlier 0.72.0-versioned pair, not the final 0.73.0 executable. See the
[phase report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE73_TEXT_LAYOUT.md)
for exact provenance and unverified native/CI/platform boundaries.

## Files layout (0.72 candidate)

Files has compact real Files/Project Text mode controls, a short workspace name
with its full path on hover, a full-width query beside Find, and one Directory
scope line when discovery is supported. A short source/status line retains
unsupported, changed, incomplete, and error information; long explanations stay
available through hover or bounded scrolling rather than changing search scope.

Without preview, results fill the available width in one column. Basenames are
primary and parent paths are muted, with file icons and the existing loaded-file
status. Original full labels and action identities are retained. Non-ASCII text
is handed intact to native single-line elision; it may hide a long suffix and is
not a guarantee of complete grapheme-boundary rendering. Hover shows the original
full label; no displayed abbreviation is used as a request path.

Preview remains explicit: select a result and use Preview or Space. Its read-only
content appears below the results. Hide preview or Escape returns to results;
hiding retires the view's intent without falsely completing a physical Read.
Preview/Open/Close placement is bounded by viewport and font metrics, not by the
number or timing of returned results. Automatic search does not start a Read.
Project Text's existing explicit search and continuous-preview behavior, Recent,
keyboard ownership, IME handling, dirty buffers, Undo, and exact return are unchanged.

The supported native minimum remains 780x540. Deterministic production-frame
checks also exercise 700x480 resilience at 13 and 18 points; these are not native
window proofs. The bounded Linux native pair used one 1178x814 viewport and the
same three-result fixture as its earlier baseline. See the
[phase report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE72_FILES_LAYOUT.md) for source/artifact attribution,
failed evidence and the remaining platform/preview/font limits.

<a id="files-search-as-you-type-069-candidate"></a>

## Files search-as-you-type (0.69 bounded acceptance)

**0.69.0 passed bounded acceptance.** Exact dual-platform CI passed all four
jobs and all three exact-source package audits passed. Linux recorded
**2,200/0/71**, Windows **2,138/0/114**, each across 50 suites; all 24 new Files
checks passed. The bounded Trust-off Linux native journey passed typing without
Enter, scope changes, explicit Preview, Open/Back, dirty Undo and cancel/reopen,
with timing and integrity verified. A click replaced the planned Down step;
that journey supplies no native Down-entry evidence. Native pixels do not
measure the fixed 250 ms interval or backend operation counts.

This changes only the **Files** provider in Find and return. **Project Text**
still searches only after explicit Enter or Find. Current-file Find remains a
separate local editor feature. The full interaction below is available offline;
the [0.69 verification report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE69_FILES_LIVE_SEARCH.md#accepted-0690-follow-up)
records acceptance separately from its unchanged original preparation history.
Linux Java Restart cleanup reported `grace_expired`, signal 9; that is forced
cleanup, not an all-graceful exit claim. Existing broad IME, native Windows/macOS,
real SSH and positive Structure/JDT limitations remain.

### Type, settle, or search immediately

1. In a connected workspace, open **Files** or press **Ctrl+Shift+N**. Check the
   provider label below to see whether Project files or the loaded-file fallback
   is available. Opening the window alone does not start a whole-project scan.
2. Enter a filename query. With Project files, optionally enter an exact
   **Directory scope**, such as `src/main`. Every committed query or scope edit
   starts a fresh **250 ms** pause for the complete current query/directory pair.
   The message **Waiting for typing to pause… Enter searches now.** identifies
   the waiting state. Repainting, caret movement and a Delete that changes no
   text do not restart the interval. A fresh committed text or paste edit gets
   a new identity and interval even if its final bytes match the prior value.
3. Leave the valid input unchanged for that interval to submit automatically, or
   press **Enter** in the query field or select **Find** to search immediately.
   Explicit submission cancels the pending timer. Repeating Enter/Find while
   that exact request is pending does not send it twice. A later deliberate
   rerun after the request finishes remains an explicit action.
4. Automatic submission and arriving results keep the current control focus.
   Use **Down** to enter the results and the ordinary selection, Preview, Open
   and Back actions below. Automatic search does not select text in the editor,
   open a file, save a draft or request content preview.

Only **one FindFiles** request can run at a time, with at most **one latest
replacement**. If an older scan is still finishing, later edits replace the
waiting candidate; they do not create one queued scan per keystroke. Both the
query and directory must still match when a request is admitted and when its
result is accepted. Editing either field immediately retires old rows, pointer
press/release tickets, Open/Preview intent and the old preview. Changing A to B
and back to A creates a fresh identity; the first A result cannot become active
again. A stale success or error cannot overwrite the new view or move focus.

### Empty input, unavailable discovery and interruptions

- **Empty query and root scope:** show only open buffers and the already loaded
  Explorer snapshot, with **zero FindFiles**. There is no automatic root scan.
- **Empty query and nonempty scope:** show **Enter a filename query before
  searching this directory scope.** It sends zero FindFiles; either add a valid
  query or clear the scope. It does not silently broaden the scope.
- **Old or unsupported peer:** keep **Open and loaded files** local filtering
  and zero FindFiles. Directory discovery remains unavailable. This fallback
  cannot find a file in an untouched directory.
- **Busy, rejected or failed request:** keep the explanation and inactive old
  results. No idle polling, automatic retry or resubmission occurs when the
  busy condition disappears. Make a fresh valid edit or explicitly select Find
  after the condition is resolved. Waiting behind an already-running Files scan
  is only the single latest replacement described above.
- **Hide, Close, Escape, provider switch, connection/context change or competing
  modal ownership:** retire the pending timer and intent. Late completion cannot
  reopen the view, restore obsolete rows or focus another control. Reopening
  does not replay cancelled work. Retiring an intent does not close the transport
  or forcibly interrupt a blocked filesystem call.
- **Interrupted timing:** **Typing pause was interrupted. Press Enter to search.**
  requires a fresh edit or explicit submission; an invalid clock does not create
  an immediate or repeated scan.

### IME composition and cancellation

Composition in the query or directory field sends no automatic request. A clear
terminal nonempty Commit can start a **full new 250 ms** interval, even when the
final text equals an earlier query. It is not permission to use the remaining
part of an older timer. Ordinary request validation still applies.

An empty Commit, Disabled without a clear terminal commit, or a new preedit
followed by an ambiguous ending/cancellation pauses automatic settlement. The
message is **Automatic search paused after composition. Edit the query or press
Find to search.** Cedar preserves the visible TextEdit query and scope exactly;
it never rolls them back, restores a saved string or rewrites the user's input.
A fresh valid committed edit or clean explicit Find can resume searching.
Neither lifecycle notifications nor elapsed time alone resume a paused query.
Active composition and mixed or stale input cannot authorize an explicit search.
This conservative rule does not establish broad native IME support.

The existing literal matching, ranking, scan budgets and **Incomplete scan**
meaning below apply unchanged to automatic and explicit submissions. The fixed
pause is an admission rule, not a response-time guarantee. No CamelCase, full
case folding, persistent index, filesystem watcher, hard cancellation or
performance advantage is implied. Preview stays explicit and adds **zero
automatic Reads**.

## Stale-navigation isolation (0.70 candidate)

**0.70 is not yet accepted.** If a Text result starts an ordinary Read and you
move elsewhere before it arrives, its bytes may still be cached in an inactive
buffer. The canceled Text line must not become the destination when you later
open that file normally from **Files** or **Recent Files**. That later Open uses
the same caret behavior as a fresh ordinary Open of the same bytes.

The candidate binds a newly created ordinary line jump to current navigation
identity and excludes a language-navigation target. It retains the exact cached
text/revision, existing valid background jumps, ordinary caret behavior, dirty
buffers, Undo and location history. It adds no automatic Read, Save, search,
retry, provider, transport or language capability.

The old 0.69 production-frame control observed a stale line-2 scalar position
of 10, versus 39 for a fresh ordinary Open of identical fixture bytes. This is a
source-derived supplied-input reproduction, not a prior native GUI observation.
The [0.70 verification report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE70_STALE_NAVIGATION.md)
records frame, strict process, native and Windows evidence separately. Final
0.70.0 local aggregate and process checks passed; exact-source CI, packages and
native verification remain pending, so the candidate is not accepted.

<a id="current-file-find-loop-068-candidate"></a>

## Current-file Find loop (0.68 bounded acceptance)

**0.68.0 passed bounded acceptance.** Exact dual-platform CI and all three
package audits passed, followed by the second Trust-off Linux native core
Find/return and dirty Undo/Redo journey, with timing and integrity verified.
The first native startup observation remains incomplete, without a product
startup-failure inference. Native cap behavior, raw affinities, wider platforms
and broad IME coverage remain unmeasured; see the
[current report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT.md#accepted-0680-bounded-baseline).
Current-file Find searches the active editor draft, including unsaved text. It
is separate from Project Text, whose results describe historical disk content.
This local flow needs no language provider or execution Trust and sends no
backend operation.

### Start from the editor

1. With the editor focused, select text and press **Ctrl+F** on Windows/Linux or
   **Cmd+F** on macOS. Find accepts the whole nonempty selection only when it is
   at most **4,096 UTF-8 bytes** and contains no CR, LF or NUL. It does not trim,
   normalize or truncate the selected text. Forward and reversed selections use
   the same complete text.
2. With no nonempty selection, Find keeps its existing query. An invalid,
   multiline or oversized selection also keeps that query and shows a reason.
   Invoking Find from another control does not copy its selection into the
   editor query. The 4,096-byte rule limits automatic seeding only; it does not
   introduce a new restriction on manually entered queries.
3. Type in the Find query field to change the query. This updates the local
   match cache without moving the editor caret or selection. Opening Find alone
   also does not select a result or add location history.
4. Press **Enter** in the query field, **F3**, or **Next** to select the next
   occurrence. **Shift+F3** or **Previous** selects the previous occurrence.
   F3/Shift+F3 work from the editor or the Find query field; each accepted action
   moves once and returns focus to the selected editor text. Use the query field
   again to change the query. Held-key repeats do not repeatedly navigate.
5. Press **Escape** to close Find and return to the editor. **Back** or
   **Ctrl/Cmd+[** returns to the departure selection of the contiguous Find loop
   while the document remains open and unchanged. Existing Forward behavior
   remains available.

### Which occurrence is selected

Matching is **literal, case-sensitive and non-overlapping**. For example, `.*`
looks for those two characters; `aaa` contains one retained `aa` match. No
regular expressions, whole-word or case options, Unicode normalization or
project-wide matching are added.

All entry points use the same selection-relative rules:

- An exact selected hit advances to its next or previous retained hit, including
  when the selection direction is reversed.
- Otherwise, Next chooses the first hit whose start is at or after the higher
  selection endpoint. Previous chooses the last hit whose end is at or before
  the lower endpoint.
- A collapsed caret exactly at a match start includes that match for Next; a
  caret exactly at its end includes it for Previous. A caret inside a hit skips
  that hit in the chosen direction before wrapping, so a sole retained hit can
  still be selected by wrap.
- Passing the last or first retained hit wraps to the first or last retained
  hit. No match leaves the caret and history unchanged. Wrapping back to the
  same already-selected sole hit also adds no history entry.

The cache is bound to the exact document, edit version, query and connection.
It retains the first **10,000 matches** in document order. At that cap, the label
says **First 10000 matches retained**, or identifies the current hit within
that retained range; it does not claim that the file has exactly 10,000 matches.
Navigation and wrap use only the retained hits. A document, query or connection
change makes the old cache ineligible for navigation.

### Return, editing and cancellation

The first successful move records one departure in existing location history;
subsequent moves in the same contiguous loop do not add one entry per hit.
**Editing, switching tabs, changing the query or connection, another navigation
command, manually changing the editor selection, and Escape end that loop.**
The next successful move starts a new departure. Ending a loop does not remove
an existing Back entry. Back still obeys the normal open-document/edit-version
checks and cannot restore an older text revision.

Find navigation changes only the caret, selection and editor scroll. It does not
edit or save the draft, reset Undo/Redo, read a file, submit Project Text search
or change its retained result state. Existing Replace remains explicit preview
and Apply under its current draft and selection checks; changing Find text
invalidates an obsolete Replace preview. Find does not apply a replacement.
Owned input is required: competing modal focus, active IME composition, mixed
input and stale press/release context cannot authorize a Find move. Clean native
IME availability notifications do not themselves compose text.

The [0.68 report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE68_CURRENT_FILE_FIND.md)
records bounded acceptance and preserves the earlier preparation evidence. The
[normal-agent proof contract](#current-file-occurrence-process-contract-068-candidate)
is a separate headless gate; neither a contract nor supplied-input frame tests
establish native Windows/macOS shortcuts, real SSH or wider language support.

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

1. Enter a query. In the **accepted 0.69 Files contract**, leave valid committed input
   unchanged for 250 ms, or press **Enter** / **Find** to submit immediately.
   **Project Text** still requires Enter or Find.
2. Press **Down** to focus the first result. Use **Up/Down** or **Home/End** to move
   through results, or single-click a row to select it.
3. With a result focused, press **Space**, or select **Preview**, to inspect it.
   In the accepted 0.66.2 contract, this also arms continuous preview for that exact Project
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

Query or Project Text filter edits make previous text results inactive; only a
new explicit successful text search activates them. Files query/scope edits
immediately retire discovery rows and preview, then follow the 0.69 settlement
rules above. Changing a field back to its earlier text does not reactivate old
rows. Switching provider retires pending intent without discarding the retained
Project Text snapshot.
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

An empty filename query with root scope shows open buffers and already loaded
Explorer files, without starting discovery. An empty query with a nonempty scope
is invalid and does not silently fall back. If discovery is unavailable, the label explicitly
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
exact original path spelling. The 0.69 bounded contract changes only Files submission
timing as described above; explicit Enter/Find remains available.

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
request runs at a time, with at most one latest candidate waiting to replace it.
In the 0.69 bounded contract, valid committed Files input can admit one scan after the
fixed pause; it never admits a content Read. Retiring a query does not disconnect
the transport or forcibly interrupt a filesystem call.

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
  contents. In the accepted 0.66.2 contract, an armed Text view can update its preview after
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

In the accepted 0.66.2 contract, preview leaves focus on the selected row in both Text
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

<a id="continuous-project-text-preview-066-candidate"></a>

## Continuous Project Text preview (0.66.2 bounded acceptance)

The accepted 0.66.2 implementation preserves this 0.66 contract. It applies
only to **Project Text in Find and return** and the bottom **FIND RESULTS** tool.
It leaves Files preview and search submission unchanged. **Space** on an owned current hit or the labelled **Preview** action
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

This feature adds no protocol, backend operation, service, index, editable
preview tab, cross-session persistence or Save action. Its validation status is
recorded in the
[0.66 correction history](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE66_CONTINUOUS_PREVIEW.md).

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
connections keep their current setup and authentication boundary. This browsing
flow does not establish new real-SSH acceptance or a sandbox against hostile concurrent
filesystem changes.

Build, Run and other existing tools remain available under their existing support
and trust requirements. Java type search remains in the Language tools and needs
an already running supported, trusted Java session. This browsing flow does not
start it automatically or provide every-language symbol search. It makes no IDEA
parity or unmeasured performance claim.


<a id="current-file-occurrence-process-contract-068-candidate"></a>

## Current-file occurrence process contract (0.68 bounded acceptance)

The versioned **0.68.0** current-artifact normal-agent Browse proof passed its
strict **57-field / 14-operation** version-5 receipt, including **zero added
current-file Find operations**. The current default-feature app and matching
normal agent were verified; source/fixture integrity and owned-agent cleanup
passed. The retained Structure **45 fields / 11 operations**, Scoped Search
**57 / ten** and Search **57 / twelve** proofs also passed. These are selected
current-artifact executions; the canonical compile drivers were not run locally.
Exact 0.68 CI, all three package audits and the second bounded Linux native
core Find journey subsequently passed. The earlier local executions remain
separate from that terminal acceptance; native scope limits still apply.

The first local normal-agent Browse integration passed on the development
snapshot still labelled **0.67.1**, before the 0.68 version bump: one exact test
passed in **0.72 seconds**. Its strict version-5 receipt contained **57 typed
fields and the same 14 backend operations**, with **zero added current-file
operations**, all six new positive observations true, unchanged source-fixture
hashes, Trust off and one owned agent reaped. Cargo JSON selected the current
default-feature app and matching normal agent; source and binary hashes were
verified. This early pass remains distinct from the later versioned local
aggregate and process results above; it does not establish exact CI, package
or native acceptance.

The version-5 contract requires **exactly 57 typed fields and 14 operations**.
All 50 version-4 fields and values remain, except the schema version advancing
to 5. The original 12-operation prefix, final two ordinary Reads, source hashes,
Trust-off condition, one owned agent, verified close/reap and 240/30/300-second
active/cleanup/watchdog bounds remain unchanged. The existing Python driver
retains its 120/180/330-second compile/backend/app bounds and its exact selected
test; no new driver, workflow job or provider is introduced.

After the retained Browse journey, the proof reuses its ordinarily loaded dirty
`origin.txt` and existing Undo history. Actual raw-input-hook/update frames must
seed Find from the selected `d`, visit the literal scalar ranges `0..1`,
`13..14` and `22..23` across the existing Unicode/CRLF draft, and wrap both ways.
The complete loop must create one Back departure. Escape returns editor focus;
Back and Forward restore the exact observed selections before any query-change
cases run. Queries `.*` and `DIRTY` must have no matches or navigation, retaining
literal and case-sensitive semantics. Find navigation must preserve draft,
revision, Undo/Redo and the retained project-search snapshot. Actual Undo/Redo
must still restore the saved source and dirty draft.

The seven new strict fields record selection seeding, next/previous wrapping,
literal no-match behavior, Escape/Back exact return, Undo/Redo preservation,
state preservation, and **zero additional backend operations**. The process
checks the empty dispatch channel throughout the local flow and compares its
exact ledger before and after; a feature that never navigates cannot pass the
positive range assertions. The Python tests reject absent, extra or incorrectly
typed fields, false observations, nonzero operation counts, appended operations
and historical version-4 receipts. A fixed digest independently retains all 50
previous fields and nested values, alongside their original prefix assertions.

This contract is headless normal-agent evidence only when executed and strictly
validated against current artifacts. It does not establish native GUI input,
Windows/macOS GUI, real SSH, JDT/provider behavior or broader language support.


<a id="files-settlement-process-contract-069-candidate"></a>

## Files settlement process contract (0.69 bounded acceptance)

The Browse **version-6** contract requires **exactly 64 typed fields and 16
operations**. All 57 prior fields remain. Restoring the old schema version and
removing only the two new ledger entries reconstructs the exact version-5
contract, including its original fourteen-operation prefix. The retained
current-file Find flow still adds zero backend operations. The only new suffix
is exactly **FindFiles("Needle", "untouched", 100)** followed by
**FindFiles("origin.txt", "", 100)**; no automatic Read is allowed.

Actual production frames must demonstrate the complete pause before admission,
latest query/directory identity, immediate old-row retirement and identical
explicit-submission deduplication. The process then exercises explicit dirty
preview, ordinary Open and exact Back while preserving dirty bytes and Undo.
The new seven strict fields attest to these observations and zero automatic
Reads; malformed, missing, extra, false or wrongly typed observations and extra
operations must be rejected. The old 57-field contract and prior prefix retain
independent equality checks, including the earlier fixed version-4 contract.

The suffix has a 60-second admission budget inside the unchanged
**240/30/300-second active/cleanup/watchdog** bounds. Source/fixture integrity,
Trust off, owned-agent shutdown/reaping and the strict receipt remain required.
The existing driver bounds and package inventory remain unchanged. An early
default-feature normal-agent run passed **64 fields / 16 operations / zero
automatic Reads** with verified cleanup, while still reporting development
version **0.68.0**. Its earlier disk-only proof-observer failure and bounded
proof-only correction remain in the 0.69 report. The later **0.69.0** selected-
artifact execution also passed **64 fields / 16 operations / zero automatic
Reads**, alongside retained Structure **45/11**, Scoped Search **57/10** and
Search **57/12** proofs. Structure uses the explicit mock; the canonical compile
drivers were not run locally. Exact dual-platform CI, all three package audits
and the bounded Linux native journey subsequently passed as recorded above.
This headless evidence itself cannot establish native timing, raw event delivery,
broad IME, Windows/macOS GUI or real SSH.


## Stale-navigation process contract (0.70 candidate)

Browse **v7** requires **74 strict typed fields / 17 operations**. The entire
v6 **64-field / 16-operation** contract remains an exact projection, including
its nested values and older v5/v4 contracts. The sole appended operation is one
ordinary Read of `untouched/deep/Needle.rs`. The harness retains its real normal-
client response until after a newer Files navigation, then verifies that exact
bytes are cached inactive without the canceled Text line jump.

Files first activates the cached document; subsequent Recent reuse must match
the separately observed fresh ordinary caret, with **zero operations** for both.
Independent Recent-first activation is covered by a separate focused frame
regression. Exact Back, dirty selection and Undo remain
required. The ten new typed observations, exact ledger, 30-second call timeout,
Trust-off state and owned-agent cleanup are strict requirements. Delaying response
delivery in the harness is not a server/network delay or native-race observation.
The unchanged process bounds remain **240/30/300 seconds** for active work,
cleanup and watchdog. The [phase report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE70_STALE_NAVIGATION.md)
records the **17/17 parser pass** and final **0.70.0** normal-agent **74/17** pass
separately from the preserved development failures. The versioned local Rust
aggregate passed **2,203/0/71 across 50 suites**; Python passed **806 total / ten
skipped / zero failed suites across 29 suites**, alongside strict Clippy and
formatting. Structure **45/11**, Scoped Search **57/10** and Search **57/12**
selected-artifact proofs also passed. Structure uses a mock; canonical compile
drivers were not run locally. Exact-source CI, packages and native gates remain
pending. These checks do not establish Windows GUI, real SSH, broad IME or
positive native Structure/JDT behavior.

## Completed Project selection feedback (0.70.1 candidate)

`Selection completed: /path` records the last successfully completed explicit
Select Current File in Project action. The path is the validated target of that
action. It can remain visible after Recent, a tab or Back activates another file;
Project is still an explicitly loaded snapshot and does not automatically follow
that navigation or issue another List/Read. The success line is truncated to one
row when needed; hover shows its complete path. Pending, cancellation and failure
messages retain their existing replacement and layout behavior.
