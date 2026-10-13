# Cedar: project browsing and navigation roadmap

Design decision, 2026-10-12, with delivery status updated for the accepted 0.66.2 baseline and the 0.67 File Structure candidate. This roadmap does not itself establish a new release or acceptance.

## Product decision and baseline

Cedar will prioritize understanding and navigating projects: finding files and text, inspecting results, following code relationships, and returning to the previous context. Integrated project Build, Run, test execution and debugger development are no longer roadmap priorities. Existing modules are retained; this decision does not remove working editing, recovery, remote access or execution features.

The interaction reference is **IntelliJ IDEA 2026.2, Windows New UI, Dark theme, Compact density and default Windows shortcuts**. This is a design reference, not a claim of pixel-perfect, platform, language or feature parity. Cedar keeps its own product identity. Linux validation does not establish Windows GUI behavior.

The original design baseline was accepted Cedar **0.59.3**. The current accepted
bounded baseline is **0.66.2**, public commit
[`a0ecbdbe27577ef83a60eee1b8af88edb0c68f35`](https://github.com/LLLLimbo/cedar-ide/commit/a0ecbdbe27577ef83a60eee1b8af88edb0c68f35),
with [CI 38392603729](https://github.com/LLLLimbo/cedar-ide/actions/runs/38392603729)
and all three package audits passed. Both systems ran 50 suites and all 17
preview tests: Linux **2,128/0/70**, Windows **2,066/0/114**. Strict Browse retained
50 fields / 14 operations, Scoped Search ten operations, Search twelve operations,
and both JDT proofs were verified. The bounded Linux native journey passed both
Text follow flows, bottom B160, Open/Back, dirty selection, Undo/Redo and
Escape/hide/reopen within its timing bounds.

**0.67 File Structure is the next CI-first candidate and is not yet accepted.**
Its scope is a current-file chooser over the existing exact outline snapshot,
with local name filtering and declaration/Back navigation. The rapid-reopen
correction passed 17 local frame checks, the Linux normal-agent/mock proof and
final local aggregate/strict checks. Exact CI and packages remain pending; no
new positive native JDT or GUI Trust-enabled result is claimed. The
0.66.0 bottom-preview failure, 0.66.1 Windows caller-count failure and unlaunched
native check, 0.65.0 glyph stop, and 0.64.0 strict timing failure remain in the
[verification report](TEST_REPORT.md#accepted-0662-bounded-baseline).
Windows/macOS GUI, real SSH, arbitrary viewports and general CJK/IME coverage
remain unverified. Original compatibility and phase decisions retain their
historical reference versions.

### Delivery decision

Phase A is an internal integration slice. **The first meaningful public browsing milestone includes Phase A plus bounded, genuine project-wide filename discovery from Phase B1.** Rearranging panels while still finding only previously loaded files is not that milestone. An early demonstration must explicitly say it is partial and name the loaded-file restriction.

The first milestone supports a complete journey: connect a workspace, find a file in an untouched directory, inspect a text result, open the intended source, preserve an existing dirty buffer, and return to the exact departure location. It does not require a persistent index, filesystem watcher, split editor, a new language server, or project execution.

## What can be reused, and what is missing

| Area | Accepted implementation | Consequence for this roadmap |
|---|---|---|
| Workspace boundary | Typed bounded operations through `cedar-client` and a stdio `cedar-agent`; relative-path validation; local and SSH connection specifications | Keep filesystem access in the backend. A UI redesign must not introduce direct frontend traversal or a second remote transport. |
| Project tree | Explicitly expanded lazy directories, admitted cached paths and keyboard navigation | Reuse loading, identity and dirty-buffer opening. Add coherent placement and focus; do not call the cache a whole-project index. |
| File chooser | Open buffers plus current/loaded Explorer directories; 64 visible results, bounded cache snapshot | Reuse selection/open behavior as fallback and recent context. Add an independent path-discovery provider for untouched directories. |
| Project text search | Literal, case-sensitive disk search, bounded traversal/read/result budgets; historical submitted query; exact captured-row opening | Preserve the engine and stale-result safety in Phase A. Add explicit filters only with a separately specified backend contract. |
| Code navigation | Definitions, references, implementations, outline, Java type lookup and parameter information | Reuse existing provider routes. Do not present Java type lookup as every-language symbol search or dependency-source navigation. |
| Return navigation | Exact location history for still-open, unchanged buffers | Preserve departure selection/affinity and dirty-buffer history. Persistent recent locations are a later feature. |
| Editing | Conditional Save, Undo/Redo, dirty tabs, recovery and bounded draft operations | Browsing must not imply Save, replace draft bytes, retarget a tab, or reset history. |

Source anchors: [navigation](../crates/app/src/navigation.rs), [Explorer](../crates/app/src/explorer_tree.rs), [Search interaction](../crates/app/src/search_keyboard.rs), [location history](../crates/app/src/location_history.rs), [workspace search](../crates/workspace/src/lib.rs), [Java type search](JAVA_TYPE_SEARCH.md), [architecture](ARCHITECTURE.md).

## Interaction architecture

### A browsing workspace, not a collection of configuration forms

Use a compact header, left Project tool window, central editor tabs and bottom Find/results tool window. The active workspace, provider scope and incomplete-result status remain visible. Configuration and execution controls should not dominate the browsing surface. Moving or hiding existing controls requires an explicit implementation decision, not deletion of their modules.

A common navigation entry presents **Files**, **Project Text**, and available **Java Types** providers. Unsupported categories are explained rather than simulated. The first internal Files provider is labelled **Open and loaded files**. Once B1 is available, distinguish **Project files** from the fallback. Empty entry initially shows existing open/recent context; it must not silently trigger a full traversal.

Use IDEA entry points where the corresponding behavior exists: Double Shift for the common entry, Ctrl+Shift+N for Files, Ctrl+Shift+F for Project Text, Alt+1 for Project, and Ctrl+Alt+Left/Right for location history. Ctrl+N or symbol shortcuts must not claim unavailable language providers. Native Tab, Enter, Escape and existing editing shortcuts must remain predictable. Search fields, results, preview and editor need explicit focus transitions; traversing dozens of unrelated controls is not an acceptable primary journey. In the initial explicit-submit contract, Enter in the query submits; Up/Down or a single result click selects; Space or the labelled Preview button requests preview; Enter on a result or a double-click permanently opens it. Hover alone does nothing. Escape from preview returns focus to the selected result; Escape from the entry dismisses it and restores the captured editor focus only while that identity remains current. Late Reads cannot reopen a dismissed view. Explicit preview was the bounded first-slice contract. The accepted 0.66.2 contract below retains explicitly armed continuous preview only for the two Text views; it does not claim identical IDEA behavior.

### Separate presentation from provider authority

Introduce a small provider adapter boundary in the frontend. It exposes a bounded query state, immutable displayed result identity, status, selection and explicit activation. It does not own a filesystem, start a service, reinterpret another provider's payload, or turn old data into current data.

Each provider keeps its own request and safety rules. File results bind workspace/connection generation, query sequence, scope, provider version and exact relative path. Text results also retain their historical submitted query and disk-result identity. Java results retain existing language session, synchronized-source and unversioned-result limitations. Switching provider or editing a query retires old activation intent. Late successes and errors cannot replace a newer view or open a different row.

This adapter work must preserve original ordered input ownership. Reuse reviewed focus and captured-result mechanisms rather than building another global event filter. Pointer hover is not evidence of click ownership; a result changing between press and release cannot retarget activation.

### Bounded preview

Preview is read-only and separate from a permanent editor tab. For an already open buffer, preview that exact draft and label it as a draft, with no backend Read. For an unopened path, one explicit preview selection may issue the existing bounded Read. Use a bounded one-entry preview cache keyed by workspace/connection generation, provider query/result identity, exact path and returned revision; open-draft previews additionally bind document ID and edit version. Discard it on a different identity. Keep one preview Read in flight and at most one replacement selection; late success/error cannot replace the latest preview. Changing selection retires old intent without disconnecting the transport. Query typing alone does not admit file content reads. Permanent opening still uses the ordinary Open path; reusing preview bytes as a newly editable document is not part of this first slice.

Keep the existing 1 MiB file limit initially. Unreadable, binary or oversized files, and changes Cedar can actually establish, show a clear state rather than a fabricated snippet. Current text-search hits carry no file revision, so preview cannot always detect changes since the search; it must not infer unchanged bytes from the absence of a warning. Preview cannot Save, alter selection/Undo in an existing editor, or assert that historical text-search coordinates still match current disk bytes. Opening a text hit reuses ordinary Open and existing history admission, retaining the distinction between historical match and current source.

## Filename discovery: chosen compatibility direction

### Choose a narrow additive optional contract

Add a dedicated read-only `FindFiles` operation and an **optional, fixed-shape top-level `file_discovery` descriptor in the Hello payload**, initially version 1. Keep protocol 4, existing AgentInfo schema, all existing capability meanings, the 32 direct-capability limit and the two-group limit unchanged. This is an explicit new extension field with its own small fixed bound, not a disguised text Search mode or an unbounded list of capabilities.

The descriptor is one optional Hello field with exactly one required member, `version: u32`. Its compact serialized object is 13 bytes for v1 and at most 22 bytes for any allowed positive u32; no strings, nested objects, arrays or additional fields are admitted. The outer descriptor field and the inner version must each occur at most once. This version is the extension schema/operation contract version, independent of protocol 4 and AgentInfo schema 1. A present descriptor object missing its version is malformed. New readers reject zero, duplicate or wrong-type versions and unexpected members. Absent or null means unavailable; a well-formed future positive version is unsupported and sends **zero FindFiles requests**. A v1 descriptor alone enables the dedicated client gate; an arbitrary flat `find_files` claim does not. Malformed metadata follows the existing fail-closed handshake policy. Availability is retained from the first valid Hello for that connection, and refreshed only on reconnect. No version string inference, trial request, or fallback to recursive frontend List is allowed.

This is chosen over a protocol-wide version-5 negotiation project because current Hello and `AgentInfo` deserialization do not deny unknown fields: old clients can ignore the new descriptor while preserving their unchanged capability lists. An old agent omits it. The implementation must prove this with the actual accepted 0.59.3 reader and serialization fixtures, not merely assert it from Serde conventions. If that proof fails, stop and review an explicit protocol transition; do not weaken old validation.

| Client / agent | Required result |
|---|---|
| Old / old | Existing behavior unchanged. |
| New / old | Existing browsing works; Project files is unavailable; no new operation is sent. |
| Old / new | Old Hello parsing and capability validation pass; existing operations behave unchanged; no new operation is invented. |
| New / new, v1 descriptor | FindFiles is enabled subject to connection identity and normal path bounds. |
| New / absent or unsupported descriptor | Explicit fallback label; no discovery requests. |
| New / malformed descriptor or oversized response | Reject under the declared validation boundary; never promote partial parsing into supported discovery. |

Relevant code: [AgentInfo and protocol limits](../crates/protocol/src/lib.rs), [capability inventory](../crates/workspace/src/capabilities.rs), [client handshake](../crates/client/src/lib.rs). Validate the one-member cardinality and the at-most-22-byte compact serialization explicitly. The version-only object has a fixed normalized shape; the existing 8 MiB frame cap still bounds raw whitespace/escaped wire spellings. Do not claim a smaller raw-wire limit without implementing it. The optional extension needs independent negative tests before coding; it must not become a generic capability escape hatch.

### B1 traversal and matching contract

Start with a **query-scoped traversal**, not persistent indexing. FindFiles reads directory metadata and returns only validated relative regular-file paths. It does not read file contents, run Git, load project build scripts, follow symbolic links or Windows reparse points, or require execution trust. Apply the path policy to ancestors as well; retain the existing limits against hostile concurrent namespace replacement rather than claiming a new sandbox. Binary and larger-than-1-MiB files may be discovered; opening still obeys Read limits.

Proposed initial limits: one nonempty single-line query of at most 256 UTF-8 bytes; root (empty relative scope) or one validated relative directory scope; paths at most 4,096 UTF-8 bytes each; at most 20,000 examined entries, depth 64, a cooperative two-second elapsed budget, 100 default / 200 maximum returned paths and 512 KiB of encoded result data. Also cap the pending-directory stack at 4,096 entries and 512 KiB, with checked accounting and streamed enumeration. Confirm the implementation meets these independent bounds before its seal; do not renew deadlines as results arrive. A cooperative deadline cannot interrupt an individual blocking filesystem call, so it is not a hard completion guarantee.

Matching initially uses case-insensitive literal filename/path matching with a documented Unicode policy and deterministic ranking: exact basename, basename prefix, basename substring, then path substring; equal ranks use a stable path tie-breaker. Retain a bounded top-result set while traversing; ranking is deterministic for the observed candidates, not a promise of globally best results after a truncated scan. No regex, typo correction, CamelCase or every-language symbol claim in v1. Display the exact original path; never normalize path spelling into a different activation target. The provider scans untouched directories within its declared scope and bounds.

Use an explicit, visible exclusion policy. The initial policy may reuse current excluded directory names (`.git`, `target`, `build`, `dist`, `node_modules`, `.idea`, `.venv`, `venv`, `__pycache__`), but must label that scope. Nested .gitignore semantics and IDE module-exclusion equivalence are not implied. Permission errors, depth/time/entry/stack/encoded-response/result limits and unrepresentable paths must yield bounded reason categories and an incomplete status. A narrower folder can address traversal cutoffs; merely narrowing query text may not. **No matches within an incomplete scan is not “file does not exist”.**

Only one discovery request may be in flight. A newer query retires its results immediately and retains at most one replacement intent; it does not queue every keystroke. Initial implementation uses explicit submission while preserving the common popup's keyboard flow. Debounced search-as-you-type may follow once remote latency and cancellation behavior are measured. Do not close a transport merely to cancel an ordinary filename query or claim hard cancellation of a blocked filesystem operation.

The response is an observation, not a filesystem lease. Result activation validates captured identity and uses ordinary Open; files may have changed since discovery. Dirty open buffers are reused without another Read and retain text and Undo. A filename-only reopen or preview preserves their selection; a text-hit activation intentionally navigates to its historical line hint and records the departure for Back. No implicit Save or retargeting occurs.

## Phases and non-goals

| Phase | Outcome | Delivery boundary |
|---|---|---|
| A: internal browsing integration | Shared provider presentation, coherent Project/Find/editor focus, bounded preview and exact return navigation over current providers | Internal or explicitly partial demonstration. No whole-project filename claim. |
| B1: first public browsing milestone | Add compatible bounded Project files discovery; complete journeys 1–10 below against an exact package | Meaningful release: a file in an untouched directory can actually be found and opened safely. |
| B2: search depth | Directory/file-mask scope, explicit case/word options, grouped persistent text results and measured query responsiveness | Each added option has specified matching and historical-result semantics. Regex is a separate decision. |
| C: semantic accuracy | Accurate Java project/dependency source navigation, then usages and hierarchy workflows | Existing JDT integration is reused; real projects, provider freshness and resource behavior are separately demonstrated. |
| Later | Persistent file index/watchers, split editors, richer recent locations and additional languages | Add only when measured user journeys justify the storage/lifecycle cost. |

Delivery status: **0.61.0** accepted same-session retained grouped text results;
**0.62.2** accepted the Directory/File masks subset and corrected same-row
activation under the [scoped-search contract](SCOPED_PROJECT_TEXT.md). Explicit
submission remains; this does not complete all B2 options. **0.63.3** added the
connected browsing shell and its narrow Alt+1 companion correction. **0.64.0**
added Recent Files with bounded functional acceptance and a preserved native
strict timing failure. **0.65.1** accepted explicit current-file Project selection
and the narrow accepted-search CJK font trigger. **0.66.2** is the latest accepted
bounded baseline, including continuous Text preview. **0.67** proposes the
CI-first File Structure slice below, without completing Phase C.

### Recent Files (0.64 bounded functional acceptance)

This slice adds a bounded path list for **This connection**. It remembers only
settled active files with known revisions, at most **64 paths**, **256 KiB of
path bytes** in total and **4,096 bytes per path**. Closing a tab retains its path,
not its text or caret. Clear, disconnect, reconnect, workspace changes and app
restart empty the list; idle repaint cannot repopulate it.

**Ctrl+E on Windows/Linux** and **Cmd+E on macOS** open Recent Files. macOS Ctrl+E
keeps its editor behavior. A local filter narrows remembered paths, with
**Open/Closed** state visible. Opening an already-live dirty file reuses its
buffer with zero Read. Opening a closed path uses ordinary Open/Read with its
default caret position. Recent Files stores and restores no closed draft, caret
or selection.

This slice adds no preview, edited-only switch, background index, protocol or
backend operation, persistence, caret history, execution permission or IDEA
parity claim. Filename discovery and project text search remain in Find; exact
return positions remain the responsibility of open-buffer location history.
See the [Recent Files guide](RECENT_FILES.md).

The final 0.64.0 local aggregate passed 2,082/0/70 across 50 suites, including
19 new Recent Files/control tests. The updated Python aggregate reported
791 total tests, zero failures and 10 skips across 28 suites; strict Clippy passed.
The final format check and current-version normal-agent proofs passed. Exact
CI and all three package audits passed; the bounded native essential functional
and integrity checks passed with the strict timing failure preserved above.
The Browse process proof
keeps its historical seven-operation prefix and all assertions, then adds the
recent closed-file reopen and dirty-buffer reuse for an eight-operation version-2
receipt. Its first caret-assumption failure is preserved; the expected cursor
now comes from the ordinary-Open baseline. A full eight-operation runtime
receipt passed before the version bump; the final 0.64.0 rerun passed the strict
29-field version-2 validator, alongside the retained ten-operation scoped proof.
The canonical compile/release pipeline was not run locally. Focused, aggregate,
process, exact CI, package and native gates are reported separately in the
[0.64 report](TEST_REPORT_PHASE64_RECENT_FILES.md).
Existing seven-operation historical evidence is unchanged.

### Accepted 0.65.1 Project selection and search CJK trigger

0.65.1 triggers the existing bounded local CJK font loader from current,
validated, accepted Project Text result paths or snippets. It adds no download,
installation, dependency, protocol or backend operation, and cannot guarantee
all glyph coverage. Exact CI and three package audits passed; the fresh bounded
Linux native journey rendered `src/层/Target.rs` readably.

The explicit visible Project action selects the active file's exact visible row
without changing Flat/Tree mode. It uses the shared single List slot and only
the known parent or ancestor chain: at most **one known-parent List in Flat** or
**32 ancestor Lists in Tree**, with a **fixed 30-second total intent deadline**.
Final selection requires current visible-row identity. Cancellation, newer input
or navigation, document and connection changes retire intent; late replies cannot
continue it or focus another row. An already-admitted List may settle under its
existing timeout without renewing intent.

The bounded native Tree/Flat, Down/Up, dirty-selection, Undo/Redo and Find-state
checks passed. Native cancellation was not exercised; its deterministic and
normal-agent checks are separate evidence. The action adds no Read, recursive or
project-wide scan, automatic follow, backend operation or protocol. Alt+1 keeps
its zero-request focus behavior. The 0.65.0 native stop at a CJK box remains a
failed earlier journey. See the
[packaged guide](FIND_AND_RETURN.md#select-current-file-in-project-0651-bounded-acceptance)
and [accepted phase report](TEST_REPORT_PHASE65_PROJECT_REVEAL.md).

### Accepted 0.66.2 continuous Text preview

Only **Project Text in Find and return** and bottom **FIND RESULTS** gain
continuous preview. A current owned **Space / Preview** action arms the exact
surface and result-view identity. First preview is immediate and leaves row
focus intact. Later real, owned selection changes must settle for **150 ms**;
repaint does not renew that deadline. Keep one preview Read in flight and only
the latest replacement candidate. Unarmed selection, hover and query typing
send zero preview Reads; a current open draft supplies preview with zero Read.
Files continues to require an explicit preview for each selection.

Render at most **41 lines / 64 KiB of UTF-8-safe line context**, with truncation
visible and the indicated line marked as historical, not a current match. If the
historical line is beyond current EOF, show unavailable rather than clamping.
The existing source Read limit stays **1 MiB**. Preview preserves draft bytes,
selection, Undo and ordinary Open/Back. A genuine same-row double-click retains
its captured-view checks.

Escape, hide/dismiss, query/filter edits, rerun, provider or surface changes,
bound document-version changes, navigation and reconnect disarm the feature and
retire pending intent. Reopening does not rearm or Read; late replies cannot
reopen a preview or steal focus. No new protocol, service, backend operation,
index, preview editor tab, persistence or Save is added. The 0.66.1 correction
requests one repaint after authorized due fulfillment produces visible preview
state; it preserves this contract. The preceding 0.66.0 passed exact CI and all three packages, with strict
Browse v4 (50 fields / 14 operations, preserving the original 38 assertions and
12-operation prefix). Its native modal follow and Open/Back journey passed, but
bottom B160 remained at “Preview requested...” and the functional gate failed.
The 0.66.1 correction passed local focused/full/strict checks and current-artifact
proofs, but three new Windows test-caller count assertions failed. The raw cause
filename was not logged; a separator mismatch is a source-level hypothesis.
No 0.66.1 native run was launched. 0.66.2 recognizes exact `browse.rs` basenames
across both path separators in test-only code; original count assertions and the
old-production negative control remain required. Production repaint logic is
unchanged. Independent review, 17 focused checks, the full 2,128/0/70 Rust
aggregate, Python aggregate, strict Clippy, formatting and current-artifact
Browse v4/scoped-search proofs passed. Fresh exact dual-platform CI, three package audits and bounded Linux native
validation then passed. Native checks covered both Text follow flows, bottom
B160, Open/Back, dirty selection, Undo/Redo and Escape/hide/reopen; they do not
establish timing intervals, request counts or raw caret affinities from pixels.
See the
[usage contract](FIND_AND_RETURN.md#continuous-project-text-preview-0662-bounded-acceptance)
and [accepted report](TEST_REPORT_PHASE66_CONTINUOUS_PREVIEW.md).

### Current 0.67 File Structure candidate

The next slice uses the existing current-file language outline in a dedicated
popup, with a platform keymap entry, bounded local literal name filtering,
source order, symbol kinds and existing hierarchy. Enter or click navigates to
the exact declaration, preserving the existing Back, dirty-draft and Undo
contract. Its interaction reference is the
[IDEA 2026.2 File Structure popup](https://www.jetbrains.com/help/idea/viewing-structure-of-a-source-file.html):
Ctrl+F12, typing a name and Enter/click navigation. Cedar does not adopt that
page’s CamelHumps, inherited members, extra sorting or framework structure.

Use only an already running provider that matches the current document and
advertises document symbols. An unavailable provider is explained; opening the
popup cannot start one or change execution trust. Reuse a current exact cached
outline; explicit invocation or Refresh may use the existing document sync and
documentSymbols request. Filtering itself sends no requests. Keep the popup’s
request destination independent of the Language tool.

Bind rows and pending activation to document, edit version, language session,
connection, snapshot and popup identity. Edits, tab or session changes, Stop,
Refresh and dismissal retire obsolete intent. Late replies cannot reopen the
popup, revive a row or steal focus. Hierarchical DocumentSymbol retains its
selectionRange and nesting; flat SymbolInformation remains explicitly
unversioned, flat and dependent on the existing checked agent URI resolution.
Do not infer local structure or improve freshness claims from a popup layout.

The round prioritizes positive frame tests and a normal-agent mock proof.
The corrected frontend passed 17 local actual-frame tests, source review and
the Linux normal-agent/mock proof with a strict 45-field / 11-operation receipt.
The first runtime’s rapid-reopen production failure and the second runtime’s
test-completion failure remain recorded. Final Rust **2,145/0/71 across 50
suites**, Python **803 total / zero failures / ten skipped across 29 suites**,
strict Clippy, formatting and retained Browse/Scoped proofs passed. Exact CI
and package audits remain pending; this is a CI-first partial result. No new
positive native JDT or GUI Trust-enabled validation is included. A later Trust-off native check, if run,
could verify only its unavailable-provider path. It would not establish positive
semantic navigation or full native acceptance. No parser, provider, protocol,
persistent index, inherited-member query or general IDEA parity is added. See
[File Structure](FILE_STRUCTURE.md) and the
[phase report](TEST_REPORT_PHASE67_FILE_STRUCTURE.md).

Connected-header **40/48** logical pixels and tools **28/34** title-row minima
remain local frame checks at **13/18** font sizes, not arbitrary native viewport
or IDEA pixel guarantees. Tool margins, separator and body add height; wrapping
or scrolling must preserve reachability. The 0.63.3 accepted bounded journey
does not validate 0.67 or erase earlier omitted narrow-window observations.

Non-goals for A/B1: new Build/Run/DAP/test-execution features; deleting existing modules; package management; arbitrary Gradle/Maven execution; multi-file refactoring; SSH deployment/credential automation; persistent indexes; complete IDEA plugin compatibility; Windows privacy integration; unrestricted large files; generic Unicode search equivalence; automatic trust; or an unmeasured “faster/lighter than IDEA” claim.

Semantic navigation still needs source roots, classpaths, dependencies and language tooling. A Java language service can index and create generated metadata even without an exposed Build/Run action. Keep this optional and explain its resource/trust boundary. Directory/filename/text browsing must remain useful without enabling it.

## Smallest coherent Phase A implementation slice

Build one **Find and return** workspace flow over existing Open/loaded Files and Project Text providers. Add a shared result/preview presentation, direct focus entry points, one explicit read-only preview, permanent Open and return to the original editor selection. Reuse the current Project tree and location history. Keep the Java Types adapter available through its existing explicit service state; do not start or expand semantic tooling in this slice.

The slice is complete only when a person can move between the two existing providers, inspect a result, open it and return using both keyboard and pointer, while an unrelated dirty tab and its Undo remain unchanged. Include query edits, late preview replies, reconnect and dismissal from the outset. The only newly admitted filesystem activity is the explicitly selected unopened-file preview Read, with a separate ledger from ordinary Open. Browsing an open dirty buffer sends zero Reads.

Do not simultaneously build persistent indexing, split panes, durable recent-location storage, new search filters or a new rendering framework. Keep egui for this first implementation and introduce a small presentation/interaction seam around the existing provider state; revisit the framework only if measured text/focus/layout limitations make the reference journeys unattainable. Complete B1 before presenting this as the new public browsing milestone.

## Twelve acceptance journeys

These are end-to-end outcomes, not counts of new buttons. Journeys 1–10 gate the first public browsing milestone. Journeys 11–12 gate their later phases; they cannot be marked passed through existing mocks alone.

| # | Journey | Required evidence |
|---|---|---|
| 1 | Connect a generated local/remote-shaped workspace with Trust off; focus Project and expand two sibling folders | Existing path boundaries and exact List ledger; keyboard and pointer routes; no service or Run operation. Real SSH remains separately gated. |
| 2 | Open Project files and find a file under an untouched nested directory | Actual agent traversal discovers the path without prior Explorer loading; scope and completeness visible; ordinary Open reads exactly the intended file. |
| 3 | Disambiguate equal filenames in different paths, Unicode paths and ranked matches | Stable result order, exact captured path, no case/Unicode path rewriting; keyboard and pointer activate the same intended item. |
| 4 | Search project text, inspect successive previews, then open a hit | Submitted-query identity remains visible; bounded preview Reads; no draft mutation; historical positions never claim current-byte freshness. |
| 5 | Preview and reopen a file with unsaved edits | Filename-only reopen/preview reuses the exact dirty buffer with zero Read and unchanged text/selection/affinities/Undo. Text-hit activation may navigate intentionally, with Back restoring the departure; no Save. |
| 6 | Navigate from an editor selection to a result, then Back and Forward | Exact departure and destination within supported history scope; no phantom entry on refusal; dirty text and history survive. |
| 7 | Change query/provider or reconnect while results/preview are pending | Late success/error and old pointer/keyboard intent cannot replace current results, steal focus or open another target. |
| 8 | Reach discovery bounds, excluded directories, unreadable paths and an old agent | Explicit incomplete/unsupported states; old peer receives zero new operations; no false no-match conclusion. |
| 9 | Complete the primary journey using the chosen IDEA-style shortcuts, Tab, Enter and Escape | Correct provider/tool focus, no editor mutation from focus-transfer keys, no conflicting shortcut replay, no long unrelated-form traversal. |
| 10 | Repeat the journey in ordinary and narrow windows with a long result list | Visible selected row, usable preview/editor, stable resize/focus; specified scale/theme screenshots. No claim that every tiny viewport is supported. |
| 11 | Use scoped text filters and retain/group a result set while browsing | Independently specified case/word/file-mask semantics, rerun versus historical results, bounded data and query identity; B2 gate. |
| 12 | Find a Java type/member, open project or dependency source, then return | Actual approved semantic producer and representative project/classpath; correct source provenance, unsupported cases and resource measurements; C gate. |

All applicable journeys run through both-OS deterministic and normal-agent tests. Native GUI evidence is exact-package and explicitly scoped. A Linux native result does not establish Windows GUI or real SSH interoperability.

## Cost-controlled engineering and validation

Develop one coherent vertical slice before a public milestone. During iteration, run focused parser/provider/input/history tests and one generated normal-agent flow. Use deterministic delayed replies and mock language services for races. Run all required aggregate, compatibility, safety, platform release, package and native checks at the integrated milestone boundary. This changes cadence, not required gates or their assertions.

Do not add an independent real-JDT job for a plain filename/layout change. Existing required JDT gates remain; new semantic producer proofs are justified when a semantic adapter or lifecycle changes. Retain failures and distinguish test fixture failures from product defects. A failed candidate is corrected at a new source revision; no blind unchanged reruns.

Before claiming an advantage, measure the same representative project and journeys: cold workspace readiness, untouched-file discovery latency, text-result/preview latency, keyboard actions to complete a task, and simultaneous frontend/agent/optional-server resource observations. Report exclusions, dataset size and incomplete scans. No comparative performance claim follows merely from using Rust.

Use a bounded resource plan for each implementation milestone and retain source, failure evidence and package provenance. Check storage and memory before large builds or native validation. Focused development checks can shorten feedback cycles, while required integrated acceptance remains unchanged. This design document is not an execution or validation receipt.

## Reference material

- [IDEA Search Everywhere](https://www.jetbrains.com/help/idea/searching-everywhere.html)
- [Find in Files](https://www.jetbrains.com/help/idea/finding-and-replacing-text-in-project.html)
- [Find tool window](https://www.jetbrains.com/help/idea/find-tool-window.html)
- [Project tool window](https://www.jetbrains.com/help/idea/project-tool-window.html)
- [Editor previews and splits](https://www.jetbrains.com/help/idea/using-code-editor.html)
- [Recent files and locations](https://www.jetbrains.com/help/idea/recent-files-and-changes.html)
- [Source-code navigation](https://www.jetbrains.com/help/idea/navigating-through-the-source-code.html)
- [New UI](https://www.jetbrains.com/help/idea/new-ui.html)
- [Project analysis](https://www.jetbrains.com/help/idea/project-analysis.html)
- [Modules and classpaths](https://www.jetbrains.com/help/idea/creating-and-managing-modules.html)

These official references describe the comparison target. Implementation coverage, tests and measured outcomes must be recorded separately; a design reference is not an acceptance receipt.
