# Verification checkpoint — 0.55.0 Tests keyboard triage

## Scope and acceptance state

[0.54.0 Duplicate line(s)](TEST_REPORT_PHASE54_DUPLICATE_LINES.md#final-accepted-0540-evidence) is the latest accepted checkpoint: public `229272889f8f8573f5a76aac8520de08b8702501`, [CI 38118831866](https://github.com/LLLLimbo/cedar-ide/actions/runs/38118831866). Its exact dual-platform results, three-package acceptance and limited cloud Linux GUI observations are archived without rewriting the earlier pending record.

0.55.0 adds keyboard triage within the existing **Tests** report reader. The implementation and acceptance coverage are authored; runtime acceptance remains pending. No Rust test, process-runtime, CI or native GUI pass for 0.55 is claimed here.

## Intended behavior and boundaries

Only an actually focused test-case row owns unmodified Up/Down/Home/End. Navigation follows the filtered rows in original XML order, clamps at either end and reveals the destination. Enter or Space on that focused row reveals its exact details and focuses **Back to case**; activating that control returns focus to and reveals the originating row. Duplicate case names remain separate through connection/session, accepted-load and original-index identity. Native Tab navigation, name/status filter controls and explicit Load/Refresh remain available.

Captured keyboard/detail-return actions must be cancelled by stale identity, changed filters, path/read/session changes or competing newer input. They must never redirect to another row merely because its label or visible position matches. Details remain historical, inert text from the accepted XML snapshot. Filtering, selection, scrolling, details and return-to-case do not add reads, writes, Run, LSP, directory scans, editor changes or guessed source navigation. Explicit report reads keep their existing operation and revision boundaries.

The existing parser limits remain: 1 MiB XML, 4,096 cases, 65,536 XML events, depth 8 and 256 KiB retained strings, plus existing attribute/detail limits. The name filter remains bounded to 128 Unicode characters and at most two report reads may be pending. No dependency, protocol or capability change is intended.

## Required acceptance

Independent review must cover actual egui focus ownership, duplicate-name identity, filtered-order clamping and reveal, detail/return routing, competing input and stale read/session cancellation. Pure and actual-frame tests must use the implemented UI paths and preserve editor bytes, selection, history and Trust-off behavior. The normal-agent process proof must run on both OSes, provide an exact operation ledger, preserve generated disk hashes and prove owned-process cleanup within its bounded deadline.

The authored process contract has 14 ordered cases and a strict 54-field receipt. Independent process source review is complete, including the final case bodies and the same 54 unique Rust/Python fields; runtime acceptance remains pending. Its generated report contains 192 cases, including 64 failed cases. The exact ledger is ten requests: four setup, three refresh/stale-read and three reconnect requests, totaling two Hello, two root List and six Read operations. Navigation, filtering, scrolling, details and Back to case must issue zero operations. Both owned agents must be observed reaped before the generated fixture is removed.

The proof retains one shared 60-second active deadline and a five-second cleanup reserve within 65 seconds total. Every Client call must admit its complete 30-second bound. The driver retains 120-second compilation, 15-second exact test selection and 90-second runtime bounds, with bounded teardown inside a five-minute CI step. These are acceptance budgets, not shipping behavior changes. The authored process proof has not been compiled or run locally; its runtime verdict requires fresh CI.

All retained CI gates and all three package audits are required on the exact candidate commit. A fresh exact-package cloud Linux GUI check must exercise focus, filtered navigation, details, return-to-case and explicit exit with Trust off and without Save, Run or LSP. A GUI observation does not establish raw internal focus/affinity, request counts, agent reaping, Windows GUI or real SSH behavior.

## Local checks and constraints

The authored regression coverage contains 17 dedicated pure/actual-frame report-keyboard tests. Retained Explorer navigation assertions are unchanged apart from passing the captured input batch through their existing call sites. The retained Duplicate line(s) process proof and driver change only their expected Cedar version from 0.54.0 to 0.55.0; predicates, case/field counts and deadlines are unchanged. None of this Rust coverage has been compiled or executed locally.

The workspace and ten Cedar lockfile package versions move from 0.54.0 to 0.55.0. A structured comparison confirms that all non-version Cedar fields and every third-party lockfile record remain unchanged. Existing packaged Windows and Linux desktop guides carry the keyboard instructions; package inventories are unchanged. These source checks do not establish implementation or runtime acceptance.

Local `cargo fmt --all -- --check` and scoped whitespace checks pass; the Cargo invocation is formatter-only. The new report-triage and retained Duplicate line(s) Python drivers pass 30 tests combined (15 each). The corrected local package/export run completed 107 tests with one host-only skip. The first invocation failed to import three test modules because `scripts` was absent from the Python import path; its initial log is preserved. The corrected invocation supplies that import path and passes. This result does not establish a Rust or process-runtime verdict.

Local Rust compilation, aggregate tests, release builds and process execution remain deferred to fresh CI. Formatting and other bounded Python/source checks must be reported with their actual results. Keep at least 200 MiB free after projected source, export and native-evidence storage. No broader cleanup is authorized. Windows recovery admission integration remains paused. The historical Git incomplete-EOF, client EOF-marker and earlier native selection-refusal causes remain unknown.
