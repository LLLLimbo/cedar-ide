# 0.63.0 browsing shell candidate

**Acceptance pending.** The latest accepted bounded baseline is **0.62.2**,
public commit
[`91aa84cb79547207ed4deb296a61834df26089ef`](https://github.com/LLLLimbo/cedar-ide/commit/91aa84cb79547207ed4deb296a61834df26089ef),
with [CI 38282992861](https://github.com/LLLLimbo/cedar-ide/actions/runs/38282992861),
all four jobs and all three exact-source packages verified. Its bounded Linux
native evidence and unrun optional checks are recorded separately in the
[current report](TEST_REPORT.md#accepted-0622-bounded-scoped-search-baseline).
The final local source is version **0.63.0**. Its aggregate, strict checks and
bounded local process proofs passed as recorded below. Exact-commit dual-platform
CI, all three package audits and native acceptance remain pending.

## Scope

The connected workspace should place **Project**, the editor and **Find** first.
The candidate reduces persistent setup chrome while keeping the existing
workspace and safety states usable:

- **Workspace** groups connection and setup controls. Disconnected, connecting,
  failed and connected states must retain their relevant actions. Errors,
  recovery notices, dirty state and connection status remain visible or directly
  reachable; compactness must not conceal an action needed to protect a draft.
- **Tools** retains entry points for **Git**, **Commands**, **Language** and
  **Tests**. The existing tools, trust gates and execution behavior remain.
- **Back/Forward** is presented in the editor tab band, using the existing
  location-history rules and disabled states.
- **Alt+1** focuses a useful enabled Project control while preserving the Find
  tool. Focus alone does not refresh, traverse, preview or open anything.
- **Ctrl/Cmd+J** restores the captured editor context when hiding tools. Reopening
  Find targets an active visible selected hit; when none is eligible, it targets
  the query field. Old, hidden, collapsed, clipped, pending or disconnected result
  identities must not gain activation authority through a focus shortcut.

The existing shared and bottom Find views retain explicit submission, one-click
selection, explicit Preview, guarded same-row double-click/Enter Open, grouped
historical results and dirty-buffer return. The
[search contract](SCOPED_PROJECT_TEXT.md) and
[usage guide](FIND_AND_RETURN.md) continue to define those boundaries. Changing
presentation cannot authorize an extra backend request or turn historical
results into current ones.

This is a frontend presentation/focus slice. There is no new backend, protocol,
search flag, search matching mode, semantic service, Build/Run feature or
execution trust. It does not claim IntelliJ IDEA parity or a performance gain.

## Geometry checks and native boundary

Actual frontend-frame tests assert connected-header heights of **40 / 48 logical
pixels** at both **780 × 540** and **1280 × 900**, using **13 / 18** font sizes.
These checks passed on the final local source. They measure Cedar's frame layout;
they do not establish native-window pixels or equivalence to IDEA.

The corresponding **28 / 34 logical pixels** for tools are **title-row minimum
heights**: enclosing margins, the separator and tool body consume additional
height. They do not describe the whole tools header or panel. Longer state
messages, larger text and wrapping may need more height; these minima are not
fixed clipping bounds.

Project, editor, Find, menus and recovery/error controls must remain reachable
through appropriate wrapping or scrolling. Hidden or fully clipped rows remain
ineligible for Preview/Open. Existing explicit tool height and **Automatic
height** behavior must still allow recovery from a cramped layout; the candidate
makes no guarantee for every tiny viewport. The exact-package native journey
must independently establish its exercised geometry and control reachability.

The optional narrow-geometry checks were not run in the fresh 0.62.2 native
journey. That earlier acceptance cannot be reused as a 0.63 native-window pass.

## Final local Linux checks — 0.63.0

The **workspace, all-target, all-feature, locked** Rust aggregate passed
**50 suites: 2,045 passed / zero failed / 70 ignored**. Its app suite recorded
**1,329 passed / zero failed / 53 ignored**. Formatting passed, as did strict
workspace, all-target, all-feature Clippy with warnings denied. The selected
Python checks passed **779 tests across 28 suites**.

The actual normal-agent **Browse** and **scoped-search** proofs passed their
existing strict runtime receipt validators with exactly **seven** and **ten
operations**, respectively. Both recorded **`shell_transition_operations = 0`**;
the shell focus transitions did not add backend operations to those journeys.
The measured transitions preserved query and accepted result-batch identity,
grouping, the selected hit, the dirty draft's full selection, Undo and location
history. Request-state comparison covers the search request and accepted batch
identity; it is not an exhaustive snapshot of pending jobs or future requests.

These local proofs invoked the compiled app test and the normal **debug-built
agent** directly through a bounded runner. The canonical CI compile, test
selection and release-driver route was **not executed locally**. The direct
runtime receipts establish their own bounded process results, not a release
build, canonical driver run, built-package audit or dual-platform CI pass.

### Preserved development failure and corrected check

An earlier coherent app run recorded **1,328 passed / 53 ignored**. A subsequent
strengthened shortcut test with ready navigation exposed a **Ctrl+J cancellation
ordering problem**. The ordering was corrected and the strengthened check was
re-run successfully; the final **1,329-test app pass** and workspace aggregate
above include that correction. The earlier successful run did not cover this
stronger condition and does not replace the preserved failed attempt.

## Verification status

| Gate | Status at this checkpoint | Evidence and remaining boundary |
|---|---|---|
| Selected Python checks | Passed locally | 779 tests across 28 suites; supporting checks, not built-package or native acceptance. |
| Final local aggregate and strict checks | Passed locally on 0.63.0 | 50 workspace suites, 2,045/0/70; app 1,329/0/53; formatting and strict workspace/all-target/all-feature Clippy passed. |
| Focus and connected-shell checks | Local checks passed; native pending | The strengthened ready-navigation Ctrl+J check passed after the preserved cancellation-ordering correction. Exact-package Project/Find/editor focus and input still require native observation. |
| Geometry and menu reachability | Frame geometry passed; native pending | 40/48 connected-header heights asserted at 780 × 540 and 1280 × 900 with 13/18 fonts. The 28/34 tools title-row minima exclude margins, separator and body. Native wrapping and control reachability remain separate. |
| Retained Browse, Search and scoped-search behavior | Local aggregate and two direct process proofs passed | Browse/scoped runtime receipts retained exact seven/ten operations and zero shell-transition operations, preserving dirty full selection, Undo and history. The canonical CI compile/selection/release-driver route remains pending. |
| Exact-commit dual-platform CI | Pending | All required jobs at the sealed public candidate; local direct process results do not substitute for the canonical CI route. |
| Three exact-source package audits | Pending | Windows development, Linux agent and Linux desktop packages matching the sealed source. |
| Bounded exact-package Linux native journey | Pending | Independently observed Project/Find/editor focus, menu access, selection/explicit Preview/genuine OS double-click Open, dirty Back/Undo/Redo, relevant geometry and safe exit. |
| Windows GUI and real SSH | Unverified | Independent platform/transport evidence; Linux native and headless Windows CI do not establish these. |

Local tests, canonical CI, package audits and native observations remain distinct
forms of evidence associated with the source actually exercised. Passing the
final local source does not establish release acceptance. Any later failure or
skipped gate must retain its own scope before a corrected execution is recorded.

## Evidence carried from 0.62.2

The accepted baseline recorded Linux **2,031/0/70** and Windows **1,969/0/114**
(passed/failed/ignored), **58 Browse**, **43 Search** and **13 scoped-search**
frame checks, and exact **7/12/10-operation** Browse/Search/scoped normal-agent
proofs. Its fresh Linux journey covered both Find views' single selection,
explicit Preview and genuine OS double-click Open, `src` / `*.rs`, group
collapse/expand, dirty-origin Back, Undo/Redo and frontend exit code 0.

The optional invalid-scope, empty-mask, broader OR-mask, edited-tuple inactivity,
modal-reopen and narrow-geometry native cases were not run. Deterministic carry
is distinct from native coverage. Earlier private native attachments were
unavailable after an environment change; recovery of public CI/packages did not
recover those attachments. The fresh native observations stand separately.

The [0.62.0 failed CI and 0.62.1 defect record](TEST_REPORT.md#0621-public-ci-passed-acceptance-blocked)
remain preserved. The 0.62.1 controlled cross-widget defect does not establish
the cause of its earlier unexpected native Open, which remains unknown. The
historical native Ctrl+Alt+Left/Right limitation is also retained. No shell layout
change alone resolves these historical evidence gaps.
