# 0.71.0 Islands-inspired main window — verification candidate

> Public-source note: raw development screenshots, logs and process receipts remain outside this source export. See [verification evidence](../PUBLICATION.md#verification-evidence).

## Subsequent CI failure

The exact public 0.71.0 commit `a78af4a72c324e4e8e8540503b1073f71ce7aeff`
failed [CI run 38507906311](https://github.com/LLLLimbo/cedar-ide/actions/runs/38507906311):
Windows export tests had three byte-oracle failures and Linux had one compact
editor menu visibility failure. There were no package artifacts; the dependent
JDT job was skipped. Local results below remain historical evidence, not release
acceptance. See the separate [0.71.1 repair report](TEST_REPORT_PHASE711_CI_REPAIR.md).

## Scope and acceptance boundary

This phase rebuilds Cedar's main-window presentation, not its editor engine or
backend. The Workspace/Tools menus, Back/Forward, actual Project/Find/Git entries,
Project controls, tree rows, document tabs and status bar share a small chrome
module. There are no placeholder Run, Debug, branch or AI actions. Original
TextEdit, document identities, history, worker requests, Trust and input guards
remain in use. Search-dialog visual redesign is reserved for the next phase.
A narrow-window preview height compatibility fix is included because the new
shared fonts exposed clipping in the existing layout.

Independent inspection of the actual r3 native frame accepted the main-window
structure as the first visual baseline. It did **not** establish whole-IDE IDEA
parity, pixel equivalence, or release acceptance. Final status corrections and
0.71.0 source verification are tracked separately below. Exact-source CI and
three-package verification are still required before publication acceptance.

## Reference and implementation

The primary macro reference was JetBrains' [IDEA 2025.3 Islands image](https://www.jetbrains.com/idea/whatsnew/2025-3/img/islands_whats_new.png).
It has macOS-style controls and unknown build, scale and zoom; it is not a
Windows 2026.2 pixel baseline. Colors sampled from images were starting points,
not claimed official UI tokens. The existing five lexical classes now use the
explicit values from JetBrains' [Islands Dark editor scheme](https://github.com/JetBrains/intellij-community/blob/e7e7972a41e572f9179f742685a9d9f739a7943b/platform/platform-resources/src/themes/islands/IslandSchemeDark.xml):
text `#BCBEC4`, keywords `#CF8E6D`, strings `#6AAB73`, numbers `#2AACB8`, comments
`#7A7E85`. Cedar still has a small lexical colorizer, not IDEA's semantic parser.

- Quiet idle menus retain real egui menu ownership and keyboard behavior.
- Project and editor headers use a common 30-point rhythm; tabs keep separate
  document-based activation/close targets and a painted overflow icon.
- Project root displays the workspace basename without changing the root path
  identity. Tree rows have a 24-point hit height and 18-point indentation;
  original width bounds, explicit expansion and bounded loading remain.
- New/Refresh/Reveal/Cancel keep four independent slots and identifiers.
- Native framebuffer clear color uses the same opaque frame color in the gamma
  space required by eframe, removing unintended black island gaps.
- Status reserves metadata/recovery before eliding the path. Idle and healthy
  status use one 28-point line. Real recovery/connection/save/cleanup risks
  remain visible; normal pending work does not move the editor vertically.
- Long notices stay one row with their complete text retained for hover; a
  4,000-character search scope must not consume the results viewport.

New sessions continue to default to **Flat**. Tree was selected explicitly in
native captures. This avoids silently widening older-peer loading behavior.

## Fonts and redistribution

The application embeds unmodified Inter Regular and JetBrains Mono Regular,
under their included SIL OFL 1.1 licenses. The UI starts at Inter 13 points;
code at JetBrains Mono 14 and gutter 13, with shared explicit code line height
of size × 1.2 and measured baseline alignment. Late system CJK fallback appends
to the same embedded base instead of resetting Latin fonts.

[Font provenance](../crates/app/assets/fonts/README.md) and the adjacent manifest
record official URLs, exact hashes, licenses and actual face versions. Inter's
upstream release is 4.1 while its font face reports 4.001; these are not conflated.
JetBrains Mono is 2.304. System CJK fonts are not installed or redistributed.

The source exporter permits only these two exact font paths and pinned hashes,
with matching manifest and complete licenses/notices. They remain byte-identical
binary blobs, marked for base64 GitHub transport; unknown/tampered binaries still
fail closed. All 343 existing dependency rows and the Apache Maven Surefire 3.5.4
fixture notice remain, alongside the new font notices. Regeneration is tested.

## Native evidence and limits

All captures used the local development fixture with Trust OFF. The native
client was 1180×812 on a 1364×1024 desktop. Independent OS scale was not queried;
no narrow native resize or Windows/macOS GUI acceptance is claimed.

The first r2 attempt never reached Connected. Its startup guard terminated the
frontend with exit -15; this is retained as a failed startup observation, not a
normal quit or functional pass. A separately authorized second attempt used the
same r2 executable (`b462b5ae672f762c53b35b8dc7cc36220592f24123abcbae973a697797d244c9`),
connected once, exercised Tree/three tabs/Java, lines 100/200, CJK→Latin, menu and
search-font observations, and exited normally. Source, fixtures and owned-process
cleanup were checked in the actual GUI namespace.

The r3 executable was
`3c01f4a58cbae102fbed982c4c507677fc51648ca9a64c07214b792b4170e9a8`.
Its final clean main screenshot was
`c4010de227a5b002780eefaa2c4d249d835d1da08b274c32181e1e40ad42fcc8`.
Startup took 30.374 seconds; cases ended 317.021 seconds after Connected and
wind-down began at 330.545 seconds. Quit-to-owned-exit took 0.540 seconds,
exit 0. All 107 production/build inputs, 44 agent inputs, font/license/archive
identities and unchanged fixture bytes were verified. No owned frontend/agent
or Cedar window remained. These captures display package 0.70.1 as development
candidates, not a released 0.71 package.

r3 visibly fixed idle menu blocks, tab baseline drift, overflow tofu/placement,
root identity, tree density, lexical colors and black gaps. It still showed the
status path centered despite Label halign. The final source corrects the outer
layout and tests actual painted glyph/response positions. Final pending-line
and long-notice corrections likewise have actual-frame regressions; the r3
screenshot does not prove these later cases.

Menu Escape closes the owned menu without leaking its dismissal into inline
Find or other owners. It does **not** restore the pre-menu editor context;
Undo in the native smoke followed an explicit editor refocus. No automatic
focus-return claim is made. Gutter divider/current-line highlighting, semantic
highlighting, right-side tool rail and additional editor surfaces remain outside
this phase. The platform-native title bar is also not an IDEA title-bar replica.

## Verification progression

The original failures remain recorded. Focused checks first caught a nested
context-lock re-entry, fractional Project right-edge clipping, exact viewport
reveal deficits and stale layout probes. Fixes retain widget identities,
input predicates and later-writer scrolling priority; no viewport enlargement
or relaxed containment tolerance substitutes for a fix.

The first complete 0.71 Rust run was **2,207 passed / 12 failed / 71 ignored**
across 50 suites. Missing old Workspace/dirty-tab text probes were updated to
actual rendered response rectangles. Full regression also found genuine footer
regressions: an unbounded long-scope notice and a separate pending row that
moved controls during asynchronous reads. Both are corrected in production;
original native pointer, double-click, saved-height and visibility assertions
are retained. A subsequent real-font check exposed a three-point Browse preview
overflow at 780×540 with 18-point text; the narrowly scoped height-budget fix
keeps the existing results/actions/preview order and native input contracts.
Final complete Rust rerun passed **2,222 / 0 / 71** (passed / failed / ignored)
across 50 suites. All 12 original failures pass again. Final current-artifact
process proofs passed: Browse v7 **74 fields / 17 operations**, File Structure
**45 / 11**, Scoped Search **57 / 10** and Search Navigation **57 / 12**. These
consume exact default-feature Cargo-JSON-selected application tests, the normal
shipping agent, and the same frozen source. File Structure uses the mock LSP;
these local runs do not claim canonical compile-driver, real JDT or desktop GUI
acceptance.

The first Browse process run stopped at a stale post-frame Cancel response.
The new right-to-left toolbar draws Cancel before Reveal starts its intent;
Cancel is enabled on the first requested repaint, while egui's post-run
`read_response` still reports the prior map. A read-only observer before
`end_pass` now directly verifies the first enabled render, stable ID/rectangle,
and the production repaint cause. Both scheduled delays were zero; exactly two
input-free observation frames left the pending List undelivered. The original
native Cancel press/release and late-reply checks pass with zero follow-up
operations. Only the test observer changed; production timing and guards did
not. The original failed run and its verified child cleanup remain recorded.

Python verification passed **818 tests / ten skipped / zero failed suites**
across 30 suites, including strict binary-export and notice-regeneration tests.
Final formatting and strict all-target/all-feature Clippy passed on the same
frozen non-Markdown source. The installed Noto CJK opt-in test also passed,
including unchanged embedded Latin metrics after fallback installation. The
final 899-file non-Markdown source map is SHA-256
`de7bb49af7ac561bc046f88457826e02b4ce46f4614e74dccdbe8f8276368273`. No
full CI or release success is asserted here until exact-source results are
recorded.
