# Verification checkpoint — 0.55.1 Tests keyboard triage fixtures

## Scope and acceptance state

[0.54.0 Duplicate line(s)](TEST_REPORT_PHASE54_DUPLICATE_LINES.md#final-accepted-0540-evidence) remains the latest accepted checkpoint: public `229272889f8f8573f5a76aac8520de08b8702501`, [CI 38118831866](https://github.com/LLLLimbo/cedar-ide/actions/runs/38118831866). 0.55.0 failed its first aggregate run on both OSes after passing strict compilation/Clippy. The exact three failures, app-suite counts, missing gates and preserved evidence digest are recorded in [the unchanged 0.55 report with appended failure evidence](TEST_REPORT_PHASE55_REPORT_KEYBOARD.md#preserved-0550-first-ci-aggregate-failures).

0.55.1 is a test-only fixture correction for the pending Tests keyboard-triage feature. No shipping input, focus, scrolling, report-read or editor behavior fix is claimed. Fresh exact-commit CI, process, package and native acceptance are still required.

## Candidate correction and limits

The new pointer fixture advanced simulated time by one second between press and release. Pinned egui permits a click for at most 0.8 seconds; the candidate gives pointer frames 0.01-second increments so those gestures meet the existing click-duration rule. Ordinary one-second settle frames stay unchanged. A new duration regression checks this distinction, and pointer probes use the actual recorded response rectangle plus current widget ID/click flags. This is a source-derived fixture explanation for the missing Load request and selection assertions. The corrected fixture has not yet established a runtime pass. No shipping click threshold or event classifier is changed.

The native Tab fixture now explicitly probes the existing overflowing scroll area's focusable target, followed by the suite header and first filtered row. It retains ordinary Tab navigation and exact semantic target assertions. The failed run's `Some(6011)` value remains an opaque logged ID; it has not been mapped to a widget by runtime evidence, so the candidate does not claim that historical identity is proven. The new probe and corrected sequence require fresh CI.

The test correction does not relax the expected report identity, filtered XML order, clamp/reveal behavior, Enter/Space details, Back to case, stale-state cancellation, explicit-read ledger, editor/history preservation or ownership checks. It does not establish a fix for insufficient window space. The 0.55 user-facing scope and all existing limits remain as documented in [the report guide](TEST_RESULTS.md).

## Required acceptance

All 18 authored pure/actual-frame report-keyboard tests and the retained workspace suites must pass on both OSes. The original 17 tests remain, including all three failed outcome assertions; one duration regression is added. The normal-agent proof remains 14 ordered cases with a strict 54-field receipt, 192 generated report cases/64 failed cases and exactly ten requests: four setup, three refresh/stale-read and three reconnect, totaling two Hello, two root List and six Read. Navigation, filtering, scrolling, details and Back to case must issue zero operations. Original generated disk hashes, complete editor selection/history and Trust-off behavior must remain intact. Both owned agents must be observed reaped before fixture removal.

The shared active deadline remains 60 seconds with five seconds reserved for cleanup within 65 seconds total. Every Client call must admit its full 30-second bound. Driver compile/selection/runtime limits remain 120/15/90 seconds with bounded teardown inside a five-minute CI step. Receipt fields, predicates and runtime budgets are unchanged.

Fresh exact-commit strict checks, complete aggregates, the report process proof, all retained gates and all three package audits are required. A fresh exact-package cloud Linux GUI check must exercise focus, filtered navigation, details, return-to-case and explicit exit with Trust off and no Save, Run or LSP. GUI observations do not independently establish raw focus/affinity, operation counts, agent reaping, Windows GUI or real SSH behavior. No 0.55 artifact exists for reuse from the failed run.

## Local checks and constraints

Only the workspace and ten Cedar lockfile package versions move from 0.55.0 to 0.55.1. A structured comparison confirms that every third-party record and all non-version Cedar fields remain unchanged. Expected source-version literals in the existing process proofs/drivers are updated separately; no dependency or package-inventory change is intended.

The 0.55.0 local formatting, Python/package results and initial import failure remain in the archived report; they are not new 0.55.1 runtime evidence. Local Rust compilation, aggregate tests, release builds and normal-agent process execution remain prohibited and unrun. Any formatter-only or Python/source checks for this candidate must be reported with their actual results; all corrected Rust runtime verdicts await fresh CI.

For 0.55.1, formatter-only `cargo fmt --all -- --check`, whitespace and structured source-equivalence checks pass. The new combined Python run passes 137 tests with one host skip: 30 driver tests and 107 package/public-export tests. Shipping Rust source is unchanged; only the two test fixtures and expected package-version literals differ in Rust. These checks do not establish any corrected Rust runtime result.

Keep at least 200 MiB free after projected source, export and native-evidence storage. No deletion or broader cleanup is authorized. Windows recovery admission integration remains paused. The historical Git incomplete-EOF, client EOF-marker and earlier native selection-refusal causes remain unknown; the 0.55.0 failures remain preserved.

## Preserved 0.55.1 process-driver failure

Public [`457cc41ed8bc3fd98db6c605bff40234b0657747`](https://github.com/LLLLimbo/cedar-ide/commit/457cc41ed8bc3fd98db6c605bff40234b0657747), [CI 38140208749](https://github.com/LLLLimbo/cedar-ide/actions/runs/38140208749), passed strict checks and complete aggregates on both platforms: Linux 1,727 passed/0 failed/65 ignored and Windows 1,670/0/109, across 48 suites each. All 18 report-keyboard tests actually passed on both OSes. The Linux job also passed the full release/default-shipping build.

Both report-triage process drivers then failed with the generic category `subprocess_nonzero`; neither retained detailed subprocess output or a success receipt. The Windows failure was logged at `2026-10-11T12:58:04.5495100Z`. The Linux driver's observed 0.35-second duration does not identify whether compilation, exact test selection or native runtime failed, and does not establish a cause on either OS.

Both jobs ended in failure. No distribution packages were produced. The only two artifacts are independent Windows Java and Maven acceptance evidence; they do not establish report-triage process, package or native GUI acceptance. The latest accepted checkpoint remains 0.54.0. The original 0.55.1 pending report and its local checks above are preserved unchanged; successful local Python checks and aggregate tests do not convert the failed CI driver into a pass.

Raw logs and job/artifact metadata are preserved under `cedar-phase551-ci-38140208749`. The Windows log SHA-256 is `14359f7a8fe78cfd1f7eaaa24451b699a56ceedf92319ce135b1e6b252bcfd03`.
