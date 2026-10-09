# Verification report · compact sidebar access / 0.26.3

This bounded follow-up makes the sidebar scroll when a 780×540 window and the
tools pane requesting its default 245-point height leave insufficient height for the explorer, selectors
and help. It retains all controls and help text, the existing action order and
ordinary-height footer placement. A constant solid outer scrollbar gutter makes
width allocation deterministic; narrow sidebars may stack the selectors.

The scope is the sidebar. Maximizing tools until they consume all remaining space,
and center-editor overflow from tabs or Find/Replace, are separate responsive
layout limitations. This change does not resize the tools pane, alter execution
trust, start services, write files, or promise simultaneous visibility of all
content in compact space. Compact success means each control and help line can
be reached and read through ordinary scrolling and keyboard focus.

## Finite acceptance

- Production panel order at 780×540 with the default requested tools height (actual content-derived bounds recorded), and ordinary
  1178×814/1320×880 sizes, across 180/246/460-point sidebar widths.
- Empty and long Unicode file lists; ordinary and larger fonts; stable gutter,
  no overlapping click targets, and every selector/help line reachable.
- Pointer selection, keyboard focus reveal through both scroll levels, first/last
  file access and resize transitions preserve selected tool, dirty draft and Undo.
- One owned cloud Linux native trust-off pass at compact and ordinary sizes.
  No Save or execution; fixture hashes and owned-window cleanup are checked.
- Exact source host/MSVC checks, full dual-platform CI and regenerated package.

## Current status

Fourteen focused renderer/state regressions passed, including nested keyboard
reveal, wheel handoff, scrollbar dragging, resize/draft/Undo preservation, and
finite recovery from zero available height. A held-thumb zero-height assertion
and a temporary-layout focus loss were reproduced and repaired before release.
The clicked-selector recovery is one-shot and yields to newer input/focus; ten
cancellation cases passed. Independent source and Context-lock review is clear.

Final host aggregate checks passed 1,054 Rust tests across 40 suites, with 29
opt-in tests ignored. Strict host and MSVC all-target/all-feature Clippy passed,
and both all-feature workspace and default-feature frontend/agent release builds
succeeded. The exact default-feature Linux binary passed the owned native trust-off
check at 780×540 with a populated report producing an approximately 253-point tools
pane, at 246- and 180-point sidebar widths. All five selectors were activated through
Tab/Enter, Shift/Tab revealed the preceding selector, inner wheel and outer
scrollbar navigation worked, and first/last Unicode files opened through pointer
and scrolling. Native keyboard reveal of the final file was not established after
Language content expanded the tools pane to approximately 357 points; that is not
counted as a passing case. Nested last-file keyboard reveal remains covered by the
headless regressions. Resizing back to
1178×814 retained the selected dirty text; focused-editor Undo/Redo restored the
expected original/draft text. All 80 initial files and the separately generated
report retained their expected hashes. No Save occurred; owned recovery and
application/terminal cleanup completed. Private screenshots are excluded.

The native 180-point case fit two rows with its actual font metrics; larger-font
stacking is covered by headless tests, not claimed as a native observation. Linux
Local uses the embedded Workspace backend. These checks do not establish Windows
GUI, trusted execution or authenticated SSH behavior.

Exact final-source dual-platform CI and a regenerated package remain required. The preceding
[0.26.2 acceptance](TEST_REPORT_PHASE26_USABILITY.md) remains the latest verified
checkpoint. Its known minimum-height limitation is preserved in that historical
report. No native Windows GUI or authenticated SSH claim is added.

## Exact checkpoint acceptance

Public `518a4409237cf654ea6d8e9a325e3ff5ed11101d` passed both operating systems in
[CI 37927403400](https://github.com/LLLLimbo/cedar-ide/actions/runs/37927403400).
All 14 sidebar regressions ran and passed on each OS, with none ignored. The four
retained tooltip tests also passed. Ubuntu aggregate: 1,054 passed/29 ignored;
Windows aggregate: 1,066 passed/82 ignored, with required opt-ins exercised later.
The required idle Java workflow matched spontaneously in 40,329 ms; recovery was
not exercised. Maven's present/missing pair passed in 30,079 ms.

The unsigned Windows development ZIP has 533 payload files plus its manifest,
all independently hash-verified against the exact source/run. Size: 5,031,657 bytes.
SHA256: `c371a05d114a22fb3c7b897bd0953d8f8af9783977c2b66a79a32276104e8314`.
The native scope and remaining tools/center layout limits above remain unchanged.
