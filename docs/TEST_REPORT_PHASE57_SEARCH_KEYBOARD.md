# Verification checkpoint — 0.57.0 Project Search keyboard navigation

## Scope and acceptance state

[0.56.1](TEST_REPORT_PHASE561_TOOLS_HEIGHT.md#final-accepted-0561-evidence) remains the latest fully accepted checkpoint: public `2b78351d0a54ae63919acc4caa7335188594a3f2`, [CI 38150096971](https://github.com/LLLLimbo/cedar-ide/actions/runs/38150096971), complete dual-platform checks and all three packages. Its tools-height native acceptance combines the original partial exact-package run with a separately approved complementary window shrink/regrow run. The original misses, partial verdict, JPEG suffix correction and all bounded GUI limits remain preserved. The earlier [0.56.0 aggregate failure](TEST_REPORT_PHASE56_TOOLS_HEIGHT.md#preserved-0560-first-ci-aggregate-failure) remains unchanged.

0.57.0 adds ordered keyboard navigation to existing Project Search results. It does not add a search engine, automatic search, background indexing, persistence, protocol/capability changes or new workspace operations. Fresh exact-commit CI, normal-agent process evidence, all three package audits and one finite fresh exact-package cloud Linux native GUI gate are **pending**. Previous checks do not establish acceptance of this candidate.

## Candidate Project Search contract

Search remains an explicit, case-sensitive literal query submitted with the existing **Search** button or Enter in the query field. The panel labels the submitted query for the accepted batch separately from the editable query. Results describe historical on-disk lines; they do not search unsaved drafts, carry file revisions or prove that a line or snippet still matches. A limit warning means the existing engine reached a bound, which may be a scan/time/response limit rather than only the 500-row limit. No results does not prove that no matching text exists outside the bounded scan.

One frontend-owned accepted batch retains at most 500 rows and 512 KiB of combined path/snippet UTF-8 bytes. An oversized response is rejected as a whole instead of silently becoming a partial batch. Rows retain backend order and original-index identity; duplicate path/line labels remain separate. A batch is bound to its accepted request and connection/workspace context. Superseded successes and errors cannot replace current search state; an actual transport disconnection still follows ordinary connection handling.

Native Tab reaches the actual enabled row controls. Only a focused result row owns a standalone, unmodified Up/Down/Home/End action: Up/Down move through accepted order, Home/End select the first/last row, boundaries clamp without wrapping, and the destination is revealed. Query and other controls retain their own keyboard behavior. Movement alone sends no request, opens no file, and does not alter editor text, selection, Undo or location history. Held-key, mixed-input, IME, modal and competing-input ownership must be covered by deterministic production-frame tests. Search pointer capture tolerates IME Enabled/Disabled lifecycle notifications alongside an otherwise clean pointer press/release. Preedit, Commit (including empty text) and previously active composition still veto that pointer action. This narrow preventive classification change does not alter keyboard/report input rules or identify the cause of an earlier native selection refusal.

Enter/Space and validated pointer activation open the exact captured row through ordinary file Open. An already open dirty document retains its text and Undo history, but activation intentionally moves its caret to the historical requested line and records the departure for Back. An unopened result follows ordinary root-confined Read and normal editor/history behavior. There is no fresh match-column or current-draft position guarantee. Once ordinary Open is admitted, existing late-read/background-tab behavior and navigation-epoch protection apply; this change does not introduce a separate cancellable search-read protocol.

Retained rows are inactive while a newer search is pending, when the input differs from the accepted query, or after connection/workspace ownership changes. Reconnect requires an explicit newly accepted search before old rows can be used. Query edits, batch replacement, connection changes, tool dismissal/switching, modal ownership and competing input cancel unfinished row intent; returning to the panel requires a fresh valid action, without promising that native focus itself is reset. Captured movement or activation must never transfer to a replacement row with the same visible label.

## Shared shipping input ownership correction

The new row handling shares raw input with existing Tests/report and Explorer
navigation. This candidate also changes their shipping input bookkeeping: each
consumer observes the same immutable, originally ordered RawInput, records a
per-event consumed mask, and filtering occurs only after all consumers inspect
that original sequence. There is no event cloning or replay, and native Tab and
unrelated keys retain normal delivery. Explorer may claim a fresh press or a key
it already owns, preventing a held key from another view from becoming a new
Explorer action. This is a production input-path change, not a test-only fixture
correction. Existing Tests and Explorer behavior requires fresh regression
coverage, including all six directional focus/hold/release transitions among
the three consumers. Source review alone does not establish runtime success.

## Required fresh acceptance

Both platform jobs must pass strict checks, complete workspace aggregates and every retained native/process gate. New deterministic tests must cover empty/single/many results, duplicate labels, Unicode, clipping/reveal, first/last-row clamps, native Tab, query-versus-row focus, Enter/Space and pointer identity, repeat release, mixed/IME/modal refusal, pending/new batches, stale success/error, reconnect and panel hide/switch. They must distinguish movement's non-mutation from activation's deliberate line jump, retained dirty bytes/Undo and Back departure behavior.

A finite Trust-off process proof on Linux and Windows uses the normal matching agent and production UI/raw-input path. Its strict 57-field receipt requires 14 ordered cases and exactly 12 requests: Hello 2, List 2, Search 6 and Read 2, with zero Write or other operations. Phase totals are setup 4, activation 1, refresh 3, reconnect 3 and empty-search 1. Movement sends zero operations; activating the dirty open result sends zero Reads, and activating the unopened result sends exactly one Read. Two generated Unicode/CRLF files provide 96 matches each (192 rows), with fixed source hashes. Explicit re-search after reconnect, unchanged source bytes and both owned agents reaped are required success predicates.

The driver binds version 0.57.0, exactly one selected ignored test and the complete typed receipt. The native proof has a 60-second active deadline, 65-second total, full 30-second request admission and five-second cleanup bound. Driver compile/selection/runtime bounds are 120/15/90 seconds with respective output caps of 4 MiB/16 KiB/256 KiB, plus bounded five-second direct-child reaping. Both OS gates use normal default-feature agents within five-minute CI steps. A separate sanitized failure artifact retains only fixed diagnostics and typed progress; it cannot establish native ownership cleanup, and direct-child cleanup never substitutes for the full success receipt. These are required contracts, not measured runtime success until CI runs. Retained report and Duplicate proof success contracts remain unchanged apart from their expected Cedar version.

One fresh exact-package cloud Linux GUI check must exercise explicit search, native Tab into rows, ordered movement and boundary/reveal, exact-row activation of an already dirty file and an unopened result, deliberate historical-line navigation with Undo/Back, query changes, hide/show and reconnect requiring explicit search. It must retain Trust off, verify fixture/package/source integrity and end with explicit discard/exit. Native observation is limited to visible behavior; deterministic/process evidence must establish internal identity, operation counts and owned-agent reaping separately. No Windows GUI, real SSH, performance or whole-product acceptance is implied. All three candidate package audits remain required.

## Local checks and constraints

The workspace version and ten Cedar lockfile package versions move from 0.56.1 to 0.57.0. Structured comparison confirms all 343 third-party dependency records, every non-version Cedar lockfile field and the three package scripts are unchanged. Existing report/Duplicate Python and Rust process-proof version expectations are updated together; byte comparison confirms those retained files differ only in version literals, with no timing, cleanup or success-predicate changes.

The prior report is preserved as the exact 7,855-byte prefix of `TEST_REPORT_PHASE561_TOOLS_HEIGHT.md`, with the final 0.56.1 acceptance appended separately. Its original SHA-256 is `1d273e91ec25232d393f8f83778c8a56f2d4772600f1014e870d3df90e477305`. New user instructions are included directly in both existing bundled entry guides; no unbundled guide link is introduced.

Source-only checks pass for the exact archive prefix, version-only edits, Python syntax, 97 repository-relative document link targets and 31 bundled guide links checked against their package inventories. The first link helper incorrectly treated the existing generated `BUNDLE_MANIFEST.json` link as a repository file and stopped; the corrected package-context check passed. All pre-existing bundled entry-guide link targets are unchanged. Focused local Python runs passed 15 Duplicate-driver tests and 47 report-driver tests, with no skips. The record is `cedar-phase57-doc-version-verification.json`.

Local Rust compilation, Rust tests/aggregates, release builds and native process execution remain prohibited and unrun; fresh Rust/runtime verdicts require CI. Other source/Python checks must be recorded with their actual outcomes. No publication or package generation is claimed here.

The combined local Python check ran 285 tests in 3.132 seconds: 277 passed and eight were skipped for platform/tool availability. It covered the new Search driver/diagnostics, retained report and Duplicate drivers, Java diagnostic collector, and all three package test families. The raw log is `cedar-phase57-python-checks.log`. The new Search driver contributes 23 tests. Workspace formatting and whitespace checks passed; formatting does not compile Rust. Twenty new `search_keyboard_*` tests and the ignored normal-agent proof are authored but have not executed locally.

Independent source reviews covered the original-event consumption mapping, all six focus/held-key transitions, exact batch admission, ordinary Open/history behavior, and process ownership/privacy/receipt bounds. Review corrected a workspace-reset key-lifetime edge and strengthened the new fixtures before any Rust execution: short native pointer timestamps, parser-valid report counts, observed overflow ScrollArea focus and the pinned one-frame Shift+Tab completion. These are source-derived corrections, not reproduced runtime failures. The process diagnostic output is failure-only so it cannot change the strict successful libtest/receipt ordering.

Keep this phase's additional retained storage within 150 MiB and at least 200 MiB free after projected source, export and native-evidence storage. No deletion or broader cleanup is authorized. Windows recovery admission integration remains paused. The historical Git incomplete-EOF, client EOF-marker, earlier native selection-refusal and Windows Java correction-witness causes remain unknown. The successful later runs do not resolve those causes; original failed logs and seals remain preserved.

## Preserved 0.57.0 first CI aggregate failures

Public commit `fb380450f0d51a0db5770c83a01416698949dd7b`, [CI 38155299046](https://github.com/LLLLimbo/cedar-ide/actions/runs/38155299046), completed with both platform jobs failed. Both passed formatter and strict workspace/all-target/all-feature Clippy checks. The Linux app aggregate reported **1,152 passed, two failed and 50 ignored in 10.81 seconds**; Windows reported **1,155 passed, two failed and 54 ignored in 26.63 seconds**. These are app-suite counts, not a successful complete workspace aggregate.

Both platforms failed the same two new tests:

- `search_keyboard_all_six_surface_transfers_preserve_original_hold_release_and_next_fresh`
- `search_keyboard_foreign_claims_cannot_promote_mixed_batch_into_search_or_other_action`

Both failures were at `crates/app/src/search_keyboard_tests.rs:747:50`, unwrapping `parse_report` with `InvalidStructure("required attribute missing")` while preparing the common report fixture. All other 18 new Search tests and all 19 retained report-keyboard tests passed on both platforms. Those partial successes do not establish the two cross-surface matrices or full candidate acceptance.

The raw error does not name the missing attribute. Source review establishes that the fixture already supplied `tests`, `failures`, `errors` and `skipped`, but omitted the mandatory `time` from both testcase elements. The production parser requires each testcase duration independently of the suite counts. This is a test-fixture diagnosis; no production input-ownership defect was reproduced or fixed, and the failed setup did not exercise the cross-surface ownership assertions.

The run produced **zero artifacts**. Release builds and downstream normal-agent/process/package gates were skipped after the aggregate failures. There is no new Search success receipt, complete retained downstream gate result, candidate package audit or native GUI evidence. The summary `cedar-phase57-ci-38155299046/verification-summary.json` has SHA-256 `67c901e3c1498bcdabbe9061dec3db936994407fee271da4de11acdeed83d34e`. The raw Linux and Windows log SHA-256 values are `bff1055fb73868e3378d0a13226e41b73dbd45fb7201b34569e6b933ff685862` and `2782f10e8d51feb2df00b88d300d3be721d142c688e60b0f0955a443ab20edb7` respectively; failed logs and earlier seals remain preserved.

The proposed 0.57.1 correction supplies `time='0'` to both testcase elements, validates the full parsed counts and exact ordered names/statuses/durations before running the unchanged matrices, and adds a negative fixture assertion that removing one time attribute returns the exact required-attribute error. It does not change the production parser, shipping input paths, original matrix assertions, engine, protocol or workflow. Fresh exact-commit CI, normal-agent proofs, all three packages and a finite exact-package Linux native GUI gate remain required. The latest fully accepted checkpoint remains 0.56.1.

The original 11,903-byte report is preserved byte-for-byte above, with original SHA-256 `f0baaa9fc6cad521f76c9e7bef3cff4c7979ff301fb4a338061522b59fa55dfe`. Its pre-CI pending statements describe the earlier state and are not rewritten by this append.
