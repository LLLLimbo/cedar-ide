# Verification checkpoint — 0.55.2 report-triage diagnostics

## Scope and acceptance state

[0.54.0 Duplicate line(s)](TEST_REPORT_PHASE54_DUPLICATE_LINES.md#final-accepted-0540-evidence) remains the latest accepted checkpoint: public `229272889f8f8573f5a76aac8520de08b8702501`, [CI 38118831866](https://github.com/LLLLimbo/cedar-ide/actions/runs/38118831866). [The preserved 0.55.0 report](TEST_REPORT_PHASE55_REPORT_KEYBOARD.md#preserved-0550-first-ci-aggregate-failures) records its three dual-platform aggregate failures. [The preserved 0.55.1 report](TEST_REPORT_PHASE551_REPORT_FIXTURES.md#preserved-0551-process-driver-failure) records successful strict checks, all 18 report-keyboard tests and complete aggregates on both OSes, followed by generic report-driver failures on both. No distribution packages were produced; the failed subprocess phase and cause remain unknown.

0.55.2 adds diagnostics only to the report-triage acceptance driver. It does not claim to fix a shipping Rust behavior or the unidentified subprocess failure. The local Python, formatter and static checks below passed; exact-commit Rust and runtime acceptance remain pending. No package from an incomplete earlier checkpoint establishes acceptance of this candidate.

## Bounded diagnostic scope

The report driver records a fixed compile, selection or runtime stage together with return-code, output EOF, cap/truncation and direct-child cleanup metadata. Failure output is restricted to at most 32 reviewed allowlisted diagnostic entries and a JSON artifact of at most 64 KiB. Unknown compiler/runtime prose, source snippets, generated fixture paths or contents, environment values and arbitrary subprocess output must not be echoed or serialized. A missing or malformed diagnostic is not a success receipt.

This diagnostic path belongs only to the report driver. The successful 54-field receipt, exact ignored-test selection, expected operation ledger and all pass predicates remain unchanged. Compile/selection/runtime timeouts remain 120/15/90 seconds. A driver cleanup result reports only what that driver actually established; it does not replace the native receipt's proof that both owned normal agents were reaped. The diagnostic change must expose uncertainty rather than infer a failed stage from elapsed time.

The existing Tests feature scope remains focused-row navigation in filtered XML order, clamp/reveal, exact Enter/Space details, Back to case and stale-state cancellation. Native Tab/filter behavior, explicit report reads, editor/history preservation and parser limits remain unchanged; see [the report guide](TEST_RESULTS.md). No dependency, remote capability, parser, package-inventory or shipping Rust behavior change is intended.

## Required acceptance

All 18 authored pure/actual-frame report-keyboard tests and the complete retained workspace suites must pass on the exact candidate on both OSes. The normal-agent proof remains 14 ordered cases with a strict 54-field receipt, 192 generated report cases/64 failed cases and exactly ten requests: four setup, three refresh/stale-read and three reconnect, totaling two Hello, two root List and six Read. Navigation, filtering, scrolling, details and return-to-case must issue zero operations. Original generated disk hashes, complete editor selection/history and Trust-off behavior must remain intact. Both owned agents must be observed reaped before fixture removal.

The native proof retains a 60-second active deadline, five-second cleanup reserve and 65-second total. Every Client call must admit its full 30-second bound. Driver compile/selection/runtime limits remain 120/15/90 seconds with bounded teardown inside a five-minute CI step. Diagnostic tests must cover sanitization, output limits, failure-stage reporting and uncertain cleanup without weakening these success requirements.

Fresh exact-commit strict checks, complete aggregates, the report process proof, all retained gates and all three package audits are required. A fresh exact-package cloud Linux GUI check must exercise focus, filtered navigation, details, return-to-case and explicit exit with Trust off and no Save, Run or LSP. GUI observations do not independently establish raw focus/affinity, operation counts, agent reaping, Windows GUI or real SSH behavior.

## Local checks and constraints

Only the workspace and ten Cedar lockfile package versions move from 0.55.1 to 0.55.2. Structured comparison confirms every third-party record and all non-version Cedar fields remain unchanged. Expected source-version literals in acceptance-only Rust and Python are maintained separately. No shipping Rust behavior change is authorized in this diagnostic-only candidate.

The final combined local Python run for 0.55.2 completed 159 tests with one host-only skip in 1.872 seconds and passed; its log is preserved as `cedar-phase552-python-checks.log`. Formatter-only and whitespace checks passed. Static comparisons verified the unchanged shared process helper and success-validator AST, version-only changes in the two Rust proof files and retained Duplicate driver, and the exact archived report prefix. Earlier local results remain in their archived reports. Local Rust compilation, aggregates, release builds and normal-agent process execution remain prohibited and unrun; these Python checks do not establish a 0.55.2 Rust or process-runtime pass.

Keep at least 200 MiB free after projected source, export and native-evidence storage. No deletion or broader cleanup is authorized. Windows recovery admission integration remains paused. The historical Git incomplete-EOF, client EOF-marker and earlier native selection-refusal causes remain unknown. The 0.55.0 and 0.55.1 failures remain preserved.

## Preserved 0.55.2 runtime visibility failures

Both complete aggregates and all 18 report-keyboard tests passed before the process failure: Linux 1,727 passed/0 failed/65 ignored and Windows 1,670/0/109, across 48 suites each. These successful aggregates do not establish process, package or native GUI acceptance.

Public [`ec019d5d6b3d5ae979378eff20137c26e3eb3470`](https://github.com/LLLLimbo/cedar-ide/commit/ec019d5d6b3d5ae979378eff20137c26e3eb3470), [CI 38142209662](https://github.com/LLLLimbo/cedar-ide/actions/runs/38142209662), produced bounded report-triage failure diagnostics on both Linux and Windows. Each identifies the failed phase as `runtime`, with return code 101, observed EOF, a complete 416-byte capture and no output or diagnostic truncation. Each driver directly reaped its child; native cleanup remains explicitly `unverified`, and no success receipt establishes that either proof's two normal agents were reaped.

Both diagnostics record the same allowlisted failure `focused report target was not revealed in its real viewport` at `crates/app/src/report_triage_process_tests.rs:816`, inside `visible_probe`. The exact calling assertion/target and its geometry are not identified by this diagnostic. The failure is known to be native proof runtime, but its precise caller and runtime cause remain unresolved. It does not retrospectively identify either 0.55.1 failed subprocess phase.

The diagnostics are preserved under `cedar-phase552-ci-38142209662`; raw child output was omitted. `report-triage-diagnostic-ubuntu-24.04.json` has SHA-256 `a058cfda5c020576c4bdbc7f98a00b047d8d5d994c9723d3dfeb6f57036be06e`, and `report-triage-diagnostic-windows-latest.json` has SHA-256 `86474a4a2f61804539b01639cd0780859a0868b1295b65c554abe69ecd109f90`. The Linux bounded-capture hash recorded within its diagnostic is `ab7e29667204ee0cafb3b608411cc08d2df6d43b976b702118044f2c4c296b21`; the Windows capture hash is `d7dfd5098259566a4c62d16adf7f77c5726e58177b39eb687d1b478641a7bd3b`. No distribution packages were produced. No cross-platform process/package/native acceptance is claimed; the latest accepted checkpoint remains 0.54.0. The original sealed 0.55.2 report and earlier failures above remain unchanged.

The final consolidated summary is preserved as `cedar-phase552-ci-38142209662/verification-summary.json`, SHA-256 `3eb3aa8d42a6e15fc175eecb2d0c6afe751daa5a8f35bd22b24891c3c6a5614c`. Both normal-agent runtime proofs failed; no distribution package or native GUI acceptance was produced.
