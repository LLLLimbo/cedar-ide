# 0.73.0 Project Text layout candidate

## Scope and preserved contracts

This stage changes the Find in Files / Project Text presentation in browse.rs
and extracts the existing filter-field renderer in search_keyboard.rs. It does
not redesign Files, Recent or bottom Find, add regex/Words/Replace, or claim
IntelliJ IDEA pixel equivalence. Query submission remains explicit and searches
case-sensitive literal disk text. Backend/protocol, scope parsing, request
admission, physical singleflight, 150 ms continuous Text preview, 250 ms Files
submission, IME ownership, stale guards, permissions, dirty drafts, Undo and
location history retain their previous contracts.

The Text-specific surface has compact mode/query/filter controls, full-width
results while preview is unarmed, basename/parent hierarchy, a separate line
number column and original accepted-index interaction identities. Group counts
and the summary count matching lines and exact-path file groups, not repeated
literal occurrences in a line. Native single-line elision receives original
Unicode text; tooltips and Read/Open use the original path and snippet.

Explicit Preview or Space shows read-only context below the results, with the
exact requested path, Disk or Current open draft source and historical-location
notice. Hide and Escape keep their existing retirement behavior. Closing and
reopening retains query, accepted results and selection, with preview disarmed.
Header, status and footer are font-derived and remain fixed across empty,
edited, pending, accepted, preview-loading/loaded/unavailable and Hide states.

The current status headline is painted outside scrolling details. A single
bounded pure Id presentation key distinguishes content/query/filter/root/
generation/accepted/pending changes; it is not request authorization and never
stores a Response or Context-owning object. Historical/error/scan details remain
scrollable. The original 4 px inter-line allowance is reused as vertical viewport
slack, with 2 px content-top spacing: details retain one measured Body line plus
4 px without changing total status/body/footer budgets.

## Development tests and independent review

The exact development visual candidate, still versioned 0.72.0, passed
**194 focused tests, zero failures, one existing ignored process test**:
Browse 97, Scoped Search 19, Text Preview 18, Search Keyboard 43 and Browsing
Shell 17. Strict app all-features/all-targets Clippy, formatting and diff checks
passed. Independent source/evidence review closed all identified blockers.
These are focused development results, not final-version full CI.

Production-base-font tests use actual first-visible and settled update frames at
1178x814 and the supported minimum 780x540, Body/Button 13 and 18 points. They
check fixed footer and actual clips across all states, full-width restoration,
original Unicode tooltip/Read identities, explicit preview/Hide, stale/inactive/
legacy results and exact accepted counts. At normal size there are 12 complete
hits plus two groups without preview; the lower preview has seven complete code
lines. Minimum-size preview retains three complete code lines after settlement.
700x480 is resilience only; at 18 points its preview may show just one complete
code line and is not a supported-size acceptance claim.

All earlier failed logs remain. Review reproduced a scrolled-away status headline
and duplicate accessible output events for one Text-tab click. An initial
identity reset still allowed native smooth-wheel tail to move the status after
rendering. The fixed headline now stays visibly clipped inside its own rectangle
through real wheel/edit/submit/waiting/accepted transitions and subsequent tail
frames, while details remain scrollable and result-list scroll remains unchanged.

A one-Body-line details attempt also failed genuinely: fractional wheel offsets
could put a complete middle row across both clip edges. Stopped-input frames did
not eliminate that zero-slack problem. The product's 4 px viewport allowance
fixed it; tests did not relax clip containment. Real point-wheel tests now require
every exact examined/read counter, incomplete reason and warning to fit wholly
inside both the painted clip and native viewport, including minimum size at both
fonts. Old tail-only/same-frame assumptions and post-layout-offset measurement
errors were diagnosed separately from this product defect.

The Text-only button helper registers WidgetInfo once with actual ui.is_enabled().
Real FocusGained and Clicked output each contain exactly one correctly labeled
Project Text Button event. Capability-disabled metadata and zero click/I/O are
dynamically checked; ancestor-disabled versus an explicit true argument is a
static ui.is_enabled() check, not a separate dynamic claim. The Files action,
surface, preview and action-control helper remain byte-identical to the old source.

## Limited native comparison and exact provenance

A separate single Linux normal-window run used the same three synthetic files,
case-sensitive cedarNeedle query, Trust off and 1178x814 client as the old baseline.
The parent actually compared the resulting native images. The retained 21 original
JPEG captures show full-width seven-line/three-group results; distinguishable
src/main.rs and tests/main.rs parents; a readable CJK fixture parent; and a lower
read-only preview with seven complete code lines and the correct target marker.
No resize, Files search, editor Open, settings change or user-computer action
occurred. Fixture CJK rendering is not arbitrary glyph coverage.

Click selects without preview; Space explicitly arms it. Subsequent Down follows
source line 4 to 5 and then the correct tests/main.rs context. Hide restores the
full list and retains selection. Escape separately disarms preview; cancel/reopen
retains query, result batch and selected tests line 4. Editing and editing back
leave old results inactive until explicit Find. One optional tests directory plus
*.rs;*.txt mask case produces two matching lines in one file. A bounded details
wheel exposes the exact accepted scan, and editing afterwards leaves the fixed
inactive headline fully visible. The approximately 570 px popup and footer remain
fixed instead of the baseline's content-dependent footer movement.

The sole run exited normally with rc0 after **470.633 seconds Connected**. Complete
postflight verified exact source/package/binary hashes, unchanged synthetic files,
all prior evidence, private storage, owned process disappearance and native window/
Terminal closure. There was no guard termination. The first post-submit and
post-Space captures were already accepted/loaded: they are first tool observations,
not instrumented first-visible paint or pending/loading/150 ms timing evidence.

This native pair is the earlier **0.72.0 development candidate**, not final 0.73.0:

- Private base: 39a90b415b0e6b0230b9eb97cfb66d090d5f046f.
- Source inventory (1,106 files):
  4fcf079c4b8a2eb217267e9075ba2c5f7b12988e5893d9cdaf9a88c564271402.
- App SHA-256:
  6f1442723327482b9e4e5795c29a72609c4b0263f7973e5eb2bd9fc14b5fb246.
- Normal agent SHA-256:
  a5c671ce958f6f1ea9096e8e6e3680ffb6bffee567344273a68076445a428205.
- Native report SHA-256:
  1d851cc592f0ba5488c2819eeb9185dda47c1696ec77ffe1093a433e0372bd1c.
- Native delivery manifest SHA-256:
  3967350e27e71707c7a08f6c902f2e24c3e8648432f62f6984f68d481e961d1f.

Raw screenshots and machine receipts stay outside the public source checkpoint.

## Final-version preparation and validation attribution

The native candidate exposed the new summary's `1 files` copy bug. Final 0.73.0
formats singular `1 matching line` and `1 file`, and plural forms for zero or two,
including incomplete observations. Exact-string tests and a production frame with
one matching line containing three literal occurrences prevent conflating words
with lines. The old shared scan-description string can still say `1 entries
examined`; it predates this layout in both Text and bottom Find and is deliberately
not changed as part of the frozen shared behavior.

Version propagation updates workspace version, the ten local Cargo.lock package
entries and existing exact process-test/validator version guards. Registry package
versions/checksums and dependencies do not change; strict schemas, canonical paths,
operation counts, compile clocks and exact byte oracles are not weakened.

Final-version Rust default-parallel workspace/all-target/all-feature tests, all
Python validator suites, formatting, strict workspace Clippy, aggregate all-feature
release and a separately built default-feature pair require fresh source-bound
receipts. The five existing canonical Browse, Scoped Search, Search Navigation,
File Structure and Report Triage drivers retain their exact source/target paths;
prewarming cannot relax their own compile or runtime bounds. File Structure uses
its existing explicit mock LSP, not a new real-JDT claim. Frozen/default/canonical
copies require before-and-after hashes for every proof.

This source report is sealed before those final execution inputs run. Their exact
results, source identity and artifact lineage are recorded externally rather than
pre-filling success counts here or changing the sealed source afterwards. Full
local validation, exact public CI, distributable package audits and any final-pair
limited native smoke are distinct gates. The earlier native pair is never
relabeled as the final version's GUI acceptance.

## Remaining limits

Normal-window development comparison is not minimum-size/large-font native
acceptance, native pending/retired/loading timing, broad IME/CJK or grapheme coverage,
Windows/macOS GUI, real SSH, or final-pair Open/Back/dirty/Undo evidence. The long
historical-details sentence is intentionally elided with details/hover access;
primary counts, current headline, group identity, preview path and controls remain
visible. No new global scrollbar policy, search state machine or fake IDE feature
was introduced to change this boundary.
