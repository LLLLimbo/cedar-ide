# File Structure — 0.67 candidate

**0.67 is not yet accepted.** File Structure is a current-file popup over
Cedar’s existing language-outline snapshot. It offers a local name filter and
explicit declaration navigation while retaining the editor’s draft and Undo
history. The corrected frontend passed **17 local actual-frame checks** and a
Linux normal-agent/mock-provider proof, plus final aggregate/strict checks.
Exact CI and package audits remain pending. This round includes no new positive
native JDT or GUI Trust-enabled validation. The accepted 0.66.2 browsing baseline does not validate
this new feature. See the
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

Use **Ctrl+F12** on Windows/Linux or **Cmd+F12** on macOS to open File Structure
for the active file. The filter matches **case-sensitive literal substrings of
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

The corrected frontend passed **17 local actual-frame checks**, source review
and a Linux normal-agent/mock-provider proof with a strict **45-field receipt /
11 Client operations**. The proof verifies cache/filter/navigation/Back/Undo
without extra operations, retired cancelled replies, unchanged fixture source
bytes and owner cleanup. It uses a controlled mock provider. The earlier
rapid-reopen production failure and later test-completion failure remain in the
phase report. Final Rust/Python aggregates, strict Clippy and formatting also
passed. Exact CI and package audits remain pending.

Windows frame execution awaits exact CI. The Linux mock result does not establish
a new positive Windows typed-Java provider journey. Existing platform language
checks retain their own scope. A possible later Trust-off native check would
cover only the unavailable-provider path, not positive structure navigation.
No full native acceptance, new real-JDT popup acceptance, real SSH, arbitrary
viewport, broad CJK/IME or IDEA parity is claimed.
