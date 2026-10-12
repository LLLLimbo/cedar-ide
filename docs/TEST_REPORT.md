# Verification checkpoint — 0.60.0 Find and return candidate

## 0.60.0 scope and acceptance state

**0.60.0 is an integrated browsing candidate, not an accepted release. Full
exact-commit dual-platform CI, all three exact-source release-package audits and
native-window acceptance remain pending.** Local focused checks establish only
their stated scope. Historical acceptance below does not transfer to these new
production paths, and no new Windows GUI or real-SSH acceptance is claimed.

The candidate combines the shared **Find and return** flow with bounded filename
discovery into untouched workspace directories. **Files** and **Project Text**
use explicit submission, immutable result activation, explicit read-only preview,
ordinary Open and existing location history. Open drafts are previewed without a
backend Read; dirty text, Undo and save state are preserved. Filename opening
retains an existing draft's selection, while a text hit uses its historical line
hint. These actions do not implicitly Save. The complete user contract is in
[Find and return](FIND_AND_RETURN.md).

Project files requires the supported optional Hello `file_discovery` version-1
descriptor. Protocol 4, AgentInfo schema 1, the 32 direct-capability limit and
two-group limit remain unchanged. Absent or unsupported extensions retain the
explicit **Open and loaded files** fallback and send no discovery requests.
Discovery reads metadata only, has a visible directory scope and exclusion
policy, reports bounded incomplete reasons, and does not create an index or
watcher. Literal filename/path matching uses Rust Unicode lowercase without
normalization or full case folding; regex, CamelCase and live search-as-you-type
are not implemented. The elapsed deadline is cooperative, not a guarantee that an
individual blocking filesystem operation can be interrupted.

Existing Build/Run and language tools are retained under their existing support
and trust requirements. No new execution trust, automatic language startup, SSH
deployment or remote transport is introduced. There is no visual-parity or
unmeasured performance claim. The browsing roadmap remains the design decision;
implementation and acceptance status are reported here separately.

## 0.60.0 local documentation and package checks

The user guide is linked from both READMEs and all three quickstarts, and is an
explicit required payload in the Windows development, Linux agent and Linux
desktop package inventories. Focused Python package/link regressions passed on
Linux:

| Family | Result |
| --- | --- |
| Windows bundle | 37 selected: 36 passed, one Windows-only junction check skipped, zero failed |
| Linux agent bundle | 31 passed, zero failed |
| Linux desktop bundle | 40 passed, zero failed |

These tests use synthetic binaries and the real guide text. They do not build or
validate current shipping binaries, substitute for the three release-package
audits, prove native GUI behavior, or establish real SSH interoperability. The
Rust/process results below come from their separate terminal runs; they are not
inferred from these documentation checks.

The local Linux **workspace, all-target, all-feature, locked** aggregate passed:
**49 suites, 1,926 passed / 0 failed / 69 ignored**. All **26 retained Python
test suites** also passed. The new browsing acceptance runner passed its **10
pure tests**, and its strict validator accepted both actual normal-agent proof
outputs. These are local results; exact-commit dual-platform CI, Windows
execution, current release-package audits and native-window acceptance remain
pending.

## 0.60.0 focused frontend verification and preserved failures

Final formatting and strict workspace, all-target, all-feature Clippy checks
passed on the final Rust sources, including the fixture-only baseline corrections.
Focused checks passed the additional Ctrl+Alt+Left/Right history aliases
and Alt+1 Project access, including positive activation, mixed-input rejection
and held-key behavior. Queued-Open and residual-input ownership checks also
passed. The aliases retain the existing bracket history shortcuts and
Ctrl/Cmd+Shift+E. The final focused run recorded **27 passed / 0 failed / 1 ignored**.
The ignored normal-agent proof was then executed explicitly using the final
0.60.0 agent and app; its exact seven-operation journey, source hashes and close
checks passed. The full local workspace aggregate passed separately as recorded
above. Neither result establishes native GUI acceptance.

An integrated focused run recorded **26 passed / 1 failed / 1 ignored**. The
failure was
`browse_tests::browse_global_entry_rejects_repeat_held_and_ime_input_without_stealing_row_activation`.
The fixture incorrectly assumed an empty native egui Preedit event was inert
while an editor selection was active; native editing deletes that selected
range. The correction collapses the selection with native ArrowRight only for
the IME case and checks the exact preconditions before the measured event.
A subsequent IME fixture assertion also failed because native Preedit changes
caret affinity. Both failures concern expected native editing behavior in the
fixture; production behavior and input guards are unchanged by the corrections.
The final focused rerun passed as recorded above; both earlier failures remain
preserved and are not reclassified as passes.

## Accepted 0.59.3 baseline and preserved pre-CI record

### Final accepted 0.59.3 evidence, verified 2026-10-12

**0.59.3 is the latest accepted bounded checkpoint.** Public commit
[`1e2a26d6dd38c17ed3723ced54797fcb43255eb6`](https://github.com/LLLLimbo/cedar-ide/commit/1e2a26d6dd38c17ed3723ced54797fcb43255eb6),
tree `d3f951aa8acd77efebad5aa42c8d773563016c5e`, passed
[CI 38192432380](https://github.com/LLLLimbo/cedar-ide/actions/runs/38192432380).
All four jobs and all three exact-source packages passed. Linux recorded
**1,862 passed / 0 failed / 67 ignored** and Windows recorded
**1,805 / 0 / 111**, each across 48 suites. Recovered public logs, package manifests
and receipts were independently rechecked on 2026-10-12; this is a recovery audit
of that original run, not a new CI execution.

Its native acceptance carries the 0.59.2 functional evidence and separate cleanup
complement through verified shipping-source equivalence. The original native
screenshots and receipts were unavailable for this audit, so their contents were
not reverified. Historical acceptance and the original 125.452-second cleanup
failure remain intact; there is no fresh native validation or Windows GUI claim.
The same-package 28.175-second complement retains its original bounded meaning.

The historical status summarized below describes the 0.59.3 source submitted on
2026-10-12 before its CI run completed. Pending statements are historical and
superseded by the accepted evidence above. The 0.59.3-only production-equivalence
and unchanged-inventory statements do not describe the new 0.60.0 implementation.

### Original scope and acceptance state

**0.59.2 is the latest fully accepted bounded checkpoint. 0.59.3 is a test-only, CI-only candidate with exact-commit CI and package verification pending.** This checkpoint adds deterministic Unicode Parameter info checks against the actual egui renderer. Production Rust and Python behavior, the workflow, dependencies, protocol/capabilities, parser limits, receipt predicates, operation inventories and all test/proof budgets remain unchanged. The workspace, Cedar lockfile records and retained exact-version assertions advance to 0.59.3.

No new native GUI run is part of 0.59.3. Its native provenance is carried from the exact accepted 0.59.2 package through a production-source-equivalence audit, with the original native limits retained. That audit does not turn authored renderer tests into executed tests, prove a new platform, or establish broad Unicode/IME behavior. The new Rust tests remain unexecuted locally and require the fresh dual-platform CI aggregate. The command-output banner remains backlog.

## Final accepted 0.59.2 evidence

The accepted public source is [`a3f69b469f16b8bd66b2f57e38aaae2bab759281`](https://github.com/LLLLimbo/cedar-ide/commit/a3f69b469f16b8bd66b2f57e38aaae2bab759281), tree `b20d684046c1bcf86c07a8db6420133b1af590af`, tested by [CI 38183040039](https://github.com/LLLLimbo/cedar-ide/actions/runs/38183040039). **All four CI jobs and all three source-linked packages passed independent verification.** Both strict checks, complete workspace aggregates and retained process/runtime gates passed. Linux recorded **1,861 passed / 0 failed / 67 ignored**, Windows **1,804 / 0 / 111**, each across 48 suites. Each platform passed the 24 Java-signature Python driver tests and the deterministic Parameter info groups: 16 frontend, 12 pure proof, six parser, five backend and two client tests.

Both independent real-JDT producers used the normal agent/normal client route and the same-run, exact-source packaged agent. Each issued exactly three Parameter info queries: first-argument and second-argument results contained **2 / 2** signatures with the exact expected active parameter, and the outside-call query returned **0**. Their exact ledgers, synchronized source, server provider support, unchanged Java source and owned cleanup were verified by the bounded headless receipts. These receipts explicitly do not prove native GUI behavior.

The three accepted archive hashes are Windows development `157836d03ba29c1d0d7915a2aa64da7bba497ad92cd07c366066c8c3c86294d1`, Linux agent `5001ef7d4efb4f0e9c994677c5588a3a5270b497549b8457b3bed1fe32d79809`, and Linux desktop `a2cfe3d921030651a34ca160fdef4c4324e24a745af5b1dd275b6658126598d3`. The Linux desktop binary is `f1e308cfffe05d67fadf88debfb3ba0809a8abc0b5a7b20c9e160871145cbe58`; its companion agent is `c4ed462427c6aff21b5ecdf0c00b45e25ca1ed5050fe7ef429078bf92697d0f3`. These are 0.59.2 artifacts, not newly built 0.59.3 binaries.

### Bounded Linux native evidence and retained first failure

The first exact-package cloud Linux native run observed both active arguments, both overload labels, an empty outside-call result, cursor/edit invalidation and explicit dismissal. An inserted `8` remained visibly selected in dirty `78` before the query and after 64 native Tab presses back to the editor, with no pointer refocus after dismissal. Undo was the next native input and restored clean `7`; the next Redo restored dirty `78` and selected `8`. Some grouped Tab sequences have only terminal screenshots; no invisible focus owner is inferred. The run observed Java graceful-exit text and frontend exit 0.

That run's strict cleanup gate **failed**: the recorded observation lasted **125.452 seconds**, exceeding the original **120-second** bound by 5.452 seconds. This failure remains failed. A separate same-package cleanup-only complement observed a dirty draft, one correct second-parameter query, normal Stop, explicit Discard/quit, frontend exit 0 and exact-window absence in **28.175 seconds**, within its separate unchanged 120-second bound. The complement supplements the acceptance scope; it does not reclassify the first attempt, repeat the functional matrix or supply new Windows GUI evidence. The complement did not open Java's graceful-exit detail before quitting, so no Java exit-code claim is made for that complement.

Both generated native workspaces retained the original 322-byte Java source, SHA-256 `2a196b0dc072953dcf36761bb91532ccbc6c64539b2d1cdcdb4857ab33045211`. JDT itself rewrote generated `.project` metadata from 282 to 675 bytes and added a 737-byte `bin/CedarParameterFixture.class`. Thus **whole-workspace equality was not met** and is not claimed; no Save or Run action explains away those explicit background-generated changes.

Acceptance combines the first functional observations and the separately verified cleanup complement. It does not establish native backend request counts, agent reaping, delayed/in-flight response races, Windows GUI, broad IME/Unicode input, restart/persistence or broad workspace stress. The headless proofs independently establish their own exact ledgers and cleanup only.

The [original 0.59.2 preparation report](TEST_REPORT_PHASE592_CANONICAL_FIXTURE.md) is archived **byte-for-byte**, 30,437 bytes, SHA-256 `957ae8fd643fa3abb161fec1ef24a1d6be24a04900e16d9a39b17ee322754ca3`. Its CI-first PARTIAL and native DEFERRED wording records the earlier preparation state, not the terminal result above. The 0.59.0 and 0.59.1 failed runs remain preserved below and in their unchanged archives. Passing 0.59.2 does not retrospectively prove that a Windows temporary-path alias caused the 0.59.1 failures; that explanation remains a source-supported, runtime-unconfirmed candidate.

## Narrow 0.59.3 renderer-test scope

The only behavioral source edit is in the test-only `crates/app/src/java_parameter_info_tests.rs` module. The existing `overload_browsing_keeps_selection_history_draft_disk_and_undo_unchanged` test is retained and strengthened with three overloads, a Unicode label containing `🦀`, `e` plus a combining mark, a literal UTF-16 parameter interval `[17, 28]` and independently checked UTF-8 byte interval `20..34`. It inspects the actual renderer's `LayoutJob` text and ordinary/emphasized sections, clicks real overload controls and returns to the first row, and preserves draft/disk bytes, full selection affinities and Undo/Redo history.

One new test, `native_signature_rendering_does_not_emphasize_missing_invalid_or_ambiguous_hints`, covers four renderer cases: no active hint, an out-of-range global hint, an out-of-range per-signature hint overriding a valid global hint, and an ambiguous repeated string label. The Parameter info frontend module now contains **17 tests**, previously 16. These are headless egui renderer checks despite the test's `native_signature` name; they are not a new native GUI run. Both tests remain **authored and unexecuted locally**, pending exact-commit CI; source inspection alone cannot establish a passing render.

Retained Duplicate, Search and report acceptance drivers, Python tests and Rust proofs change only fixed product-version literals. The existing Java-signature Python driver's synthetic version fixture also advances by literal substitution only. No production rendering code, shared parser, backend/client implementation, acceptance predicate, budget, deadline, retry, CI workflow or package inventory is changed. The existing 0.59.2 native provenance is inherited through this bounded production-equivalence claim; no 0.59.3 GUI launch is claimed or scheduled.

## Final accepted 0.58.4 evidence

The accepted source is public [`9e925cc4dc084b3893bf7ed0cc907990bb22047b`](https://github.com/LLLLimbo/cedar-ide/commit/9e925cc4dc084b3893bf7ed0cc907990bb22047b), tree `f6d4a32e4db06f192c95071065d08a1d873f1ac9`, tested by [CI 38174023016](https://github.com/LLLLimbo/cedar-ide/actions/runs/38174023016). Both strict checks, complete workspace aggregates, retained process gates and all three package audits passed. Linux recorded 1,818 passed / 0 failed / 66 ignored; Windows recorded 1,761 / 0 / 110; each completed 48 suites. Each platform passed 15 editor-entry, 27 resize, 23 sidebar, 33 Search and 19 report-keyboard tests. The normal-agent report proof matched 15 cases / 55 strict typed fields / 10 requests / two owned agents reaped. Search retained 14 cases / 57 fields / 12 requests / two owners reaped.

The exact-package fresh cloud Linux GUI run observed genuine divider dragging, pointer reset, report load/clear in automatic mode, a later genuine drag, and keyboard focus followed by Space reset. From the disabled reset onward, 37 real forward Tab presses were captured separately. The critical editor-entry witness is step **56**, followed by immediate Undo at **57** and Redo at **58**, with no intervening pointer refocus. Dirty four-line text and full selected block survived Tab; Undo restored the original clean two-line text and full selection; Redo restored the dirty duplicate and selection. Step 55's anticipatory filename is not the editor-entry witness. Several earlier focus stops lacked visible highlights and have no inferred owner. No Save, Run or LSP action was used, and source/fixture disk bytes remained unchanged.

This is bounded visible evidence for one ASCII CRLF draft, one generated report and one fresh cloud instance. It does not establish Windows GUI, raw event/frame ownership, backend operation counts or native agent reaping, broad IME/Unicode behavior, OS resize or Tools restart coverage. The pinned-egui immediate-next-Tab lock-registration limitation and lack of a cross-frame held-key lineage guarantee remain.

The [original 0.58.4 candidate report](TEST_REPORT_PHASE584_TAB_BOUNDARY_FIXTURE.md) is archived unchanged: 20,338 bytes, SHA-256 `966b95ef62c5c505837b2302d3abf33272eef2f4aa39f58dfaf5638afdb59340`. Its pending statements describe the earlier preparation checkpoint, not the terminal result above. The archive retains the 0.58.3 eight-case boundary-focus fixture failure, the 0.58.2 measured Tab mutation, all earlier failed reports, and the exact distinction between test preparation and shipping behavior. No prior failed run or native limit is erased.

## Preserved 0.59.0 first CI outcome

The tested public source is [`653c75063394677f8e8aceca3f73fcf24acf51b4`](https://github.com/LLLLimbo/cedar-ide/commit/653c75063394677f8e8aceca3f73fcf24acf51b4), tested by [CI 38180274662](https://github.com/LLLLimbo/cedar-ide/actions/runs/38180274662). The [original 0.59.0 preparation report](TEST_REPORT_PHASE59_PARAMETER_INFO.md) is archived byte-for-byte: 14,593 bytes, SHA-256 `05dafba387dd5209e607dfc5d31dfe47b7b79162ea068184f0fea4cf3b64ad72`. Its pending statements describe preparation before that CI attempt; they are not a terminal CI result or evidence for 0.59.1.

Windows selected 19 tests in `scripts/test_java_signature_acceptance.py`; five failed before the Windows strict Rust compilation, complete workspace aggregate, runtime gates and package production. The raw failures were:

- `test_short_python_child_success_nonzero_flood_and_timeout`: the real tiny Python child's output was `b"ready\r\n"`, while the test asserted `b"ready\n"`.
- `test_native_schema_and_static_fixture_digest_match_the_driver`: the decoded source fixture hashed to `3cfb97f63ad83bbcc66b9a9b4b3cc657155238337f63c3f9194d82e2f76ff91b`, rather than the required `2a196b0dc072953dcf36761bb91532ccbc6c64539b2d1cdcdb4857ab33045211`.
- `test_both_platforms_exact_stage_budgets_and_no_offline_or_bundled_workflow`: the mocked driver returned failed status rather than passed.
- `test_every_failed_stage_is_terminal_and_original_failure_preserved`: the mocked runner recorded zero calls rather than the expected first call.
- `test_failed_native_record_is_retained_and_changed_source_cannot_pass`: the expected probe record was absent.

The last three raw assertions did not log the driver's failing stage. A source-only reproduction decoded the same Rust fixture bytes with explicit UTF-8 and cp1252: UTF-8 reconstructs 322 bytes and the required `2a196b...45211` hash; cp1252 reconstructs 331 bytes and the observed `3cfb97...ff91b` hash. The old Java release parser accepts an LF fixture and rejects the corresponding CRLF fixture. The mocked release file used platform-translating `write_text`, and the parser runs before the first mocked child call. A Windows CRLF fixture causing those three downstream assertion failures is therefore a source-supported inference, not a directly logged runtime stage.

Linux strict checks, all-feature release compilation and the complete aggregate passed in the same 0.59.0 run: **1,861 passed / 0 failed / 67 ignored across 48 suites**. The new deterministic coverage passed: 16 frontend, 12 pure proof, six parser, five backend and two client tests. These passes do not execute the ignored three-query real-JDT Parameter info proof.

The Linux job then failed in **Agent/tool protocol chains** at `scripts/capability_smoke.py:74`: the exact symmetric difference was `{'language_java_signature_help'}`. The preceding stdio protocol smoke passed. Both the retained Python `TYPED_JAVA` inventory and its independent test inventory still described 31 direct capabilities, omitting the 32nd operation already advertised by the Rust implementation. This is an acceptance-inventory omission; no new Rust capability is introduced by the 0.59.1 correction. Later Linux runtime/package gates were skipped. Windows also never reached its package gates.

The separate real-JDT Parameter info producer depends on the whole `test` job matrix and was **skipped** after those prerequisite failures; neither platform producer ran. The completed run produced **zero artifacts**: no Windows development, Linux agent or Linux desktop package was produced or accepted, and no real-JDT Parameter info receipt exists for this run. The workflow's failure, Windows's five Python assertions, the Linux smoke failure and all skipped gates remain failures/skips; no unchanged retry or replacement result is substituted.

## Preserved 0.59.1 terminal CI outcome

The tested public source is [`ecdb06ead210f5c47b99dd47ccef661dcd7d7248`](https://github.com/LLLLimbo/cedar-ide/commit/ecdb06ead210f5c47b99dd47ccef661dcd7d7248), tested by [CI 38181570382](https://github.com/LLLLimbo/cedar-ide/actions/runs/38181570382). The [original 0.59.1 preparation report](TEST_REPORT_PHASE591_WINDOWS_TEXT_BOUNDARIES.md) is archived byte-for-byte: 22,570 bytes, SHA-256 `aaf0d62b6e786f440f2c762318981630571c682af51cbedc710e90ea045db4a4`. Its pending statements describe preparation before that CI attempt; its local 298-test pass does not establish Windows-host success.

The Windows Python gate selected 22 tests and recorded **six failure records**. Four belong to the single success-path method's platform × release-ending subcases; they are not four additional selected tests:

- `test_both_platforms_exact_stage_budgets_and_no_offline_or_bundled_workflow`: all four mocked `linux`/`windows` × LF/CRLF subcases returned failed status, logging `compile/driver_error`.
- `test_every_failed_stage_is_terminal_and_original_failure_preserved`: the next-stage fixture recorded five runner calls rather than the expected six, so it did not reach its intended selection-stage failure.
- `test_failed_native_record_is_retained_and_changed_source_cannot_pass`: the expected partial probe record was absent; the fixture did not reach the intended runtime receipt write.

This advances beyond the 0.59.0 failures at UTF-8 source decoding, CRLF release parsing and deliberate tiny-child byte output: their dedicated tests no longer failed in the 0.59.1 Windows run. It does not establish success for the remaining driver fixture. The six records do not print the underlying validator exception or either path's spelling. The original `compile/driver_error`, five-versus-six call mismatch and absent probe remain failed evidence.

Source inspection identifies a specific mismatch candidate: `DriverTests.exercise` originally constructs its owned base with `Path(temporary)`, then puts that spelling in synthetic Cargo metadata. The retained `compiled_test` validator requires `executable.resolve(strict=True) == executable`. A noncanonical temporary-root alias could therefore fail at this exact compile boundary before selection. The real driver's entry point already derives its checkout root from `Path(__file__).resolve().parent.parent`. This is a source-supported fixture-alias candidate, not a logged Windows-path diagnosis or a production-root correction. The fixture must meet the existing strict validator contract; the validator is not relaxed.

The terminal Linux job passed strict checks, the complete aggregate (**1,861 passed / 0 failed / 67 ignored across 48 suites**), release compilation and all retained runtime/process gates. Both the Linux agent and Linux desktop packages were produced and independently verified against this exact 0.59.1 source. These are retained-gate and partial package results for 0.59.1; they do not establish the new independent Parameter info proof, a Windows package or 0.59.2 acceptance.

The Windows failure blocked its strict Rust compilation, complete workspace aggregate, runtime gates and package production. The separate real-JDT Parameter info producer depends on the whole `test` matrix, so **both platform producer proofs were skipped**. No Windows package or real-JDT Parameter info receipt exists for this run. The workflow remains failed despite the successful Linux job and its two verified packages. The six Windows failure records, skipped producer proofs and missing Windows package remain preserved; the Linux evidence cannot substitute for them or for positive native acceptance.

## Explicit Java request and compatibility

The optional `language_java_signature_help` operation is the 32nd direct capability on the supported typed-Java isolated-agent routes. Protocol 4, capability schema 1 and the 32-name limit remain unchanged; existing Maven capability groups are retained. Older agents can omit this capability and continue serving the retained operations. Capabilities do not confer execution trust.

A request requires execution trust, a running production typed Java session, a workspace-contained `.java` document synchronized as Java, a positive exact acknowledged document version, the new backend capability, and actual server signature-help support. Generic LSP or a validation host cannot become production Java merely by claiming JDT identity or provider support. Server support accepts a provider object, with boolean `true` accepted for compatibility; no arbitrary server callback is added. Dispatch uses the captured UTF-16 cursor and exact synchronized version. Unavailable or stale identity must not send a speculative query.

Both basic-Java and Maven initialization builders set `java.signatureHelp.enabled` to true and disable `java.signatureHelp.description.enabled`. This controls the explicit server feature; it does not turn typing into a query or start a server. Existing synchronization, import and trust boundaries remain. Server-side diagnostics and ordinary startup behavior are not new Parameter info callbacks.

## Bounded plain-text results

The agent and desktop share one bounded parser: at most 32 signatures, 64 parameters per signature, 8 KiB of UTF-8 per retained label, and 128 KiB of total retained UTF-8 label text. Null and an empty signature array mean no result. Malformed shape, invalid label/offset types, invalid UTF-16 boundaries and exceeded limits reject the complete result. UTF-16 parameter offsets must be ordered and within the signature label; surrogate-pair splits are rejected. A zero-length range yields no visible highlight. A string parameter label must be nonempty and occur in the signature; only one occurrence, including overlapping matches, permits a highlight. Ambiguous matching produces no guessed range. Documentation and extension fields are not retained or interpreted; displayed text is inert.

A valid server `activeSignature` chooses the initial row. Otherwise the UI can display row zero as a local fallback, clearly distinct from a server-selected overload. A signature's own `activeParameter` takes precedence when present; an in-range top-level hint is used only for the initially displayed signature when that signature has no own hint. A present but out-of-range per-signature hint yields no active parameter and cannot fall back to the top-level hint. Missing or unusable hints do not invent parameter zero. This deliberately differs from LSP 3.17's default-to-zero selection rules and is documented in the guide. Browsing overloads is a local display action, not another query or a claim that the browsed overload is the server choice.

The frontend captures the source document, edit version, cursor, workspace/connection and language session. Changed identity invalidates a pending or displayed result. Navigation, newer queries, hiding the view, explicit Dismiss parameter info and session changes cannot revive an old response. Requesting or browsing help must preserve draft bytes, selection, Undo/Redo, saved baseline and disk files. Its status remains a historical server answer for the captured source position; synchronization does not establish index freshness or compiler correctness.

Ordinary pre-query synchronization retains its existing job and global-error semantics. Dismissal cancels the Parameter info intent and rendering, while an already submitted synchronization can still finish or report a failure through the existing language status behavior. Existing synchronization identity checks remain unchanged; this checkpoint does not claim stronger filtering of ordinary global error messages. That outcome cannot revive the dismissed panel, send its cancelled query or apply a stale Parameter info result. No tagged synchronization job, new retry or synchronization suppression mode is introduced.

## Unchanged independent CI proof contract

The retained Parameter info proof has its own CI job and bounded driver. It does not replace or loosen any existing gate, its deadline, assertion or receipt schema. The job has a 35-minute limit. The driver budget is 28 minutes: compile 600 seconds, Java preparation 180, checks 35, test selection 15 and runtime 780, totaling 1,610 seconds with 70 seconds reserved for failure teardown (20), hashing/final receipt (30) and orchestration (20). The 35-minute job allocates 60 seconds each to checkout, Python setup, Rust setup, download, package verification and receipt upload, plus the 1,680-second driver, totaling 2,040 seconds. Exact test selection and success receipts are required; merely finding a test executable or returning a process status cannot establish the proof.

The Rust proof reserves 570 seconds for active work and 180 seconds for cleanup inside the driver's runtime bound. Its fixed ordinary setup inventory is Hello 1, List 1, Read 1 and Java Begin 1. Startup polling is bounded by 1,024 polls, a 50 ms interval and one nonrenewing 75-second startup window; setup controls are bounded to 30 seconds each. The intended positive inventory is Open 1, Parameter info queries 3 and Close 1, each with a 75-second bound. Stop and owned-process reaping must finish within the separate cleanup bounds; a failure in active work cannot suppress cleanup or produce success. This exact proof contract passed for 0.59.2 as recorded above; fresh 0.59.3 receipts remain pending.

Required fresh evidence includes exact-commit strict checks and complete workspace aggregates on both platforms, all retained normal-agent/process gates, the unchanged real-JDT Parameter info proof, and source-linked Windows development / Linux agent / Linux desktop package audits. Retained deterministic parser, backend and UI coverage must continue to establish capability/trust/version gating, malformed and excessive payload rejection, UTF-16 and ambiguous-label handling, conservative active hints, stale replies and local dismissal. The bounded 0.59.2 Linux native evidence is separate from these headless gates; 0.59.3 adds no new native run.

## Source/version, package and local verification boundaries

The workspace and exactly ten Cedar lockfile records move from 0.59.2 to 0.59.3. All 343 non-Cedar package records, every dependency edge and the 11 other crate/example manifests remain unchanged. Cargo dependency resolution is not run. Nine retained Duplicate/Search/report driver, test and proof files plus the Java-signature synthetic test fixture change only exact version literals. The three package source inventories and existing entry-guide link order remain unchanged: Windows 23 links, Linux agent five and Linux desktop 14. No JDK, JDT, new binary or third-party dependency is bundled.

This preparation used Python/static checks and formatter-only inspection. It included **no local Rust compile/runtime, JDT startup or new native GUI run**. Those checks did not establish CI, package or native acceptance.

## 0.59.3 local verification and remaining acceptance

The recorded local Python run selected **321 tests: 320 passed, one expected Windows-only skip, zero failures**, in **5.887 seconds**. It covers the same 13 suite families as the 0.59.2 preparation, including all 24 Java-signature driver tests and retained/package suites. These are Python/static checks using synthetic payloads and mocked Rust/JDT paths; they do not execute the new Rust renderer tests or establish Windows-host, real-JDT or native behavior. The formatter-only workspace check, AST parsing of all 64 Python scripts and diff-whitespace check passed. No local Rust compile/test/runtime was used.

The recorded source-equivalence review identified only the renderer test change and product-version updates; production behavior, workflows, validators, budgets and dependencies were unchanged. At this preparation stage, the renderer tests still required exact-commit CI execution. Acceptance required strict checks, full workspace aggregates, retained runtime gates, real-JDT producer proofs and all three exact-source package audits. Carrying 0.59.2 native evidence required verified production equivalence and retained its first cleanup failure and all original limits. These preparation checks alone did not establish 0.59.3 CI, package or fresh native acceptance.

The byte-identical 0.59.2 archive retains all earlier local verification results, including its 321 selected / 320 passed / one expected skip Python run, the source/version audits and the original CI/native pending state. Those historical local checks do not substitute for new 0.59.3 execution.
