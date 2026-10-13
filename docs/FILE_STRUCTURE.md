# File Structure — 0.67.1 menu candidate

**0.67.1 is not yet accepted.** It adds **Tools → File Structure…** as a visible
entry to the existing current-file popup, local name filter and declaration
navigation. The menu uses the same provider, draft, Back and Undo boundaries.
The existing **Ctrl+F12** on Windows/Linux and **Cmd+F12** on macOS are unchanged;
this addition does not establish a native shortcut repair.

The original **0.67.0** passed exact dual-platform CI and all three package
audits, but its first bounded Linux native Ctrl+F12 attempt showed no popup.
A separate instrumented local probe recorded no F12 press and one release at
Cedar’s raw-input hook; that observation does not establish an operating-system
interception or the cause of the earlier shipping result. The 0.67.1 local
aggregate/strict checks, **32 File Structure frame tests** and current-artifact
process proofs passed. Independent source/input review passed without blocking
findings. Exact CI, package audits and the native menu path remain pending. Positive native symbols and a real-JDT popup
journey remain unmeasured. The latest
accepted bounded baseline is **0.66.2**. See the
[phase report](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/TEST_REPORT_PHASE67_FILE_STRUCTURE.md).

## Provider and request boundary

File Structure needs an already running language provider that matches the
active document and advertises document-symbol support. If the provider is
absent, stopped, mismatched or unsupported, the popup explains the limitation.
Invoking it never starts a language service or turns execution trust on.

A still-current exact outline snapshot can be reused. Without one, explicit
invocation requests the current document outline through the existing document
synchronization and `documentSymbols` route. **Refresh** explicitly requests a
new snapshot. This can synchronize the current editor draft to the running
agent-side language service under the existing language-session rules. Typing
in the local filter does not send requests or rebuild an index. There is no
background provider discovery, parser or implicit service startup.

The popup owns its request destination separately from the Language tool.
Opening or refreshing File Structure must not switch that tool to Outline or
reveal it. The existing Language outline remains available through its own
explicit controls.

## Choose a declaration and return

Choose **Tools → File Structure…** for the active file. The existing
**Ctrl+F12** on Windows/Linux and **Cmd+F12** on macOS remain available in the
keymap, with native shortcut delivery still unproven. The visible entry opens
the same popup and unavailable-provider explanation; it cannot enable Trust or
start a provider. The filter matches **case-sensitive literal substrings of
symbol names**, with at most **256 Unicode scalar-value characters**. It does
not search details, containers or paths, normalize Unicode or change source
order. Plain **Up/Down** and **Home/End** choose among matching rows; **Tab**
retains ordinary control traversal. **Enter** or an owned click activates that
exact declaration and dismisses the popup when the action is accepted.
**Escape** or **Close** dismisses it without navigating. Matching does
not add inherited members, CamelCase/CamelHumps, regular expressions, custom
sort orders or symbols absent from the accepted outline.

Rows preserve the snapshot’s source order, symbol kind, existing details and
hierarchy indentation. Duplicate or overloaded names remain distinct rows;
filtering cannot retarget one row to another equal-looking name. Local
hierarchical symbols navigate with their validated `selectionRange`. The normal
navigation path records the departure for **Back** and preserves unsaved bytes
and Undo/Redo. Back retains its existing restriction to still-open documents
whose edit version has not changed. Selecting a declaration intentionally moves
the caret; it does not save, replace or reload the draft. Use the existing
**Ctrl/Cmd+[** Back command or its **Ctrl+Alt+Left** alias to return. Escape
restores the still-current source editor’s focus and selection.

The interaction reference is
[IDEA 2026.2 File Structure](https://www.jetbrains.com/help/idea/viewing-structure-of-a-source-file.html),
which documents Ctrl+F12, narrowing by a typed name, and Enter/click navigation.
Cedar chooses a bounded literal filter and existing provider data. This is not
an IDEA feature or keymap parity claim.

## Freshness, bounds and flat results

The popup binds its rows and pending actions to the exact connection, language
session, document, edit version, outline snapshot and popup generation. Editing
the document, switching tabs or sessions, stopping the provider, refreshing or
dismissing retires obsolete intent. A late success or error cannot reopen a
closed popup, revive stale rows, retarget an activation or steal focus. A new
explicit invocation is needed after invalidation.

The existing outline limits remain **2,000 nodes**, **depth 32** with root depth
zero, and **512 KiB of retained text**. Hierarchical `DocumentSymbol` keeps its
nesting, checked source ranges and selection containment. Invalid or mixed
hierarchical/flat data is rejected under the existing parser boundary; a popup
does not grant malformed data navigation authority.

Flat `SymbolInformation` stays flat with the warning **“Flat symbol targets are
unversioned and may have changed”**. Its
container text is display context, not an inferred hierarchy. These locations
use the existing checked `LanguageResolveUri` request on the agent; the frontend
cannot interpret a remote URI as a local path. Existing workspace/path and
current-target range checks still apply. A freshly received unversioned result
does not prove that the provider indexed the current bytes. See the
[language-service contract](https://github.com/LLLLimbo/cedar-ide/blob/main/docs/LANGUAGE_SERVICES.md).

File Structure keeps no cross-session filter, outline or location store. It
adds no provider, protocol, backend operation, source parser, automatic trust,
Save or project-wide symbol index. Use [Find and return](FIND_AND_RETURN.md) for
filename and project-text browsing.

## Validation boundary

The original 0.67.0 exact CI passed all four jobs, including all **17 File
Structure actual-frame checks on each platform**, and all three package audits.
Its Linux normal-agent/mock-provider proof passed a strict **45-field receipt /
11 Client operations**, covering cache/filter/navigation/Back/Undo, cancelled
replies, unchanged fixture bytes and owner cleanup. A controlled mock provider
and supplied frame input do not establish native event delivery or a positive
native typed-Java journey. The earlier rapid-reopen production failure and
later test-completion failure remain in the phase report.

The 0.67.0 Trust-off native entry gate failed when Ctrl+F12 showed no popup.
The subsequent complete, loss-free instrumented trace covers **520 finished
frames**, **zero F12 presses** and **one release** at the raw-input hook. It does
not prove an upstream delivery or interception cause, replay the earlier
shipping input, or promote the shipping feature to native acceptance.

The separate 0.67.1 local checks passed **32 File Structure actual-frame tests**
(**17 retained plus 15 menu cases**) and **40 workspace-access tests**. The
versioned aggregate passed **2,160 Rust tests / zero failures / 71 ignored in
50 suites** and **803 Python tests / zero failures / ten skipped in 29 suites**,
plus strict Clippy and formatting. Current default-features artifacts passed
File Structure **45 fields / 11 operations**, Browse **50 / 14**, Scoped Search
**57 / ten** and Search **57 / twelve** process proofs. These controlled frame
and selected-artifact results are separate from the independent source/input
review, which passed without blocking findings. Exact CI, packages and native
menu validation remain pending.

Any later Trust-off native observation can validate only opening and dismissing the unavailable
popup through the menu, with the existing draft/focus protections. It cannot
validate positive structure navigation, a real-JDT popup or shortcut delivery.
No full native acceptance, Windows/macOS GUI, real SSH, arbitrary viewport,
broad CJK/IME or IDEA parity is claimed.
