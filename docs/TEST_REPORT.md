# Verification report · coherent workflow guidance / 0.26.2

This bounded polish corrects disabled explanations for **Find Java type** and
**Check Maven model**, and reconciles current setup/navigation/report/merge guidance.
The two controls use the pinned egui disabled-hover API. A native-observed sidebar
overlap is corrected with two measured tool rows, retaining all five selectors and
four keyboard-help lines. Larger fonts use measured stacked rows when necessary.
Execution permission,
capability checks and operation dispatch rules are unchanged.

The preceding [0.26.1 exact acceptance](TEST_REPORT_PHASE26_IDLE.md) passed both
platforms on public `4c22638b35f6ad1abdede1576994080c3793a617`. Its required idle Java
workflow matched spontaneously; recovery was not exercised. Earlier failed trials
and the strict opt-in resource experiment retain their original verdicts.

## Finite checks

- Actual egui pointer hovering must display each disabled reason without dispatch.
  Active controls retain their existing behavior and do not display a stale reason.
- Sidebar rectangle/nonoverlap/click checks cover 180, 246 and 460 point widths,
  including a long explorer and larger-font fallback. Full production order with
  Tests open is checked at 1178×814 and 1320×880; 780×540 covers the isolated
  sidebar only. A targeted native recheck follows the observed overlap
  at the default-width sidebar and a narrow sidebar.
- Current English/Chinese guidance distinguishes Windows typed Java/Maven from
  generic POSIX language sessions, index queries from synchronized editor actions,
  on-disk models and historical reports from unsaved drafts, and manual draft merge
  from automatic watching or saving. Report button labels match the interface.
- One owned cloud Linux native Local session stays trust-off. It exercises keyboard
  navigation, existing report loading/filtering/details and malformed input, and
  draft merge Preview/Cancel/Apply/Undo/Redo. File hashes check that no Save occurs;
  one separately recorded external fixture edit supplies the merge input.
- Linux Local GUI evidence is separate from synthetic headless Windows capability
  fixtures. It cannot verify actual Windows-only forms, trusted Java GUI usage,
  authenticated SSH or the user's computer.

A preexisting vertical-fit limitation remains: a 780×540 window with a 245-point
tools pane open cannot fit the complete sidebar header, controls, help and explorer.
That combination is not a passing full-layout claim; increase window height or
reduce/close the tools pane. Windows GUI behavior is not established by Linux checks.

## Current status

The final host checks passed 1,045 Rust tests across 40 unit/integration/example
suites; ten documentation-test suites also passed with no tests. Thirty opt-in or
diagnostic tests were ignored. The separate minimum-height diagnostic was run to
confirm the documented overlap; it is not counted as passing full-layout coverage. All four new disabled/active pointer-hover regressions and five sidebar bounds,
scaling, pointer, keyboard and production-order regressions passed. They compare
an unhovered frame with actual pointer-hover frames and verify painted explanations,
inert disabled clicks and usable active controls. Synthetic pointer appearance does
not establish a particular native hover-delay timing.

Strict host and MSVC all-target/all-feature Clippy passed. Python suites passed:
export 2, capabilities 5, Git fixture 6, Maven cache 11, resource observer 79 and
GC collector 36; bundle 28/29 with one skip, collector 80/85 with five skips, Maven
predicates 5/7 with two skips. Independent source/guidance review found no remaining
blocker. Both the all-feature workspace release and default-feature frontend/agent
release built successfully.

The first cloud Linux trust-off pass completed the planned navigation/report/merge
flows and exposed a sidebar Tests/help overlap. Its owned cleanup and file-hash verification passed: only the separately recorded
external fixture edit changed disk contents. The rebuilt default-feature candidate passed the targeted native recheck at
1178×814: default and 180-point sidebars kept all five selectors and four help
lines separate, each selector opened its panel, and Tests explicitly loaded the
expected report and retained it across panel switches. The fixture hashes stayed
unchanged, no Save occurred, and owned application/terminal windows were closed.
Final-source aggregate checks and both release builds include the repair.
Exact final-source CI and a regenerated package remain required. Private screenshots
and generated fixture evidence are not included in the public source.
