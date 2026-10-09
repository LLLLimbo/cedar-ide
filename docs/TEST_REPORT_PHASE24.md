# Verification report · explicit Java type navigation / 0.24.0

This checkpoint adds explicit standard workspace/symbol queries on an already
running trusted Java session, with actual provider gating, bounded inert results
and existing root-confined navigation. See [scope and limitations](JAVA_TYPE_SEARCH.md).

The previous 0.23 source `edfbf41aaea6633f20e0fbaf8316c8459507e895` passed
[exact Ubuntu/Windows CI 37893145529](https://github.com/LLLLimbo/cedar-ide/actions/runs/37893145529).
The explicit report-read test executed on both platforms with seven reads and all
preservation/cleanup witnesses. Its verified Windows package contained 530 payloads.
The historical pre-publication report is retained [here](TEST_REPORT_PHASE23.md).

## Finite acceptance

- Optional operation and complete lifecycle checks; no implicit language startup,
  writes, command execution, recursive scan or repeated query loop.
- Actual workspaceSymbolProvider support and bounded query/result validation,
  rejecting incomplete/malformed/oversized results without partial success.
- Query, connection and language-session identity; invalidation after edits,
  dismissal, restart or reconnect. Navigation retains dirty buffers and Undo.
- Existing normal-agent native Java Quick acceptance gains one generated unopened
  type witness, exact URI/range, empty negative query and ordinary read navigation.
  Existing process budgets, shutdown truthfulness, source integrity and prior gates remain.
- Comprehensive capability inventories include normal and nonshipping hosts and the
  extracted trust-off bundle, preserving strict equality and rejection checks.

## Current verification status

The final host aggregate passed 980 Rust tests across 40 suites, with 24 explicit
opt-in tests ignored. All 18 new chooser tests passed, including real egui frame
Search/Enter/Escape/row activation, late query/resolve/read rejection and rendered
selection with actual Document Undo/Redo. Strict host and MSVC all-target/all-feature
Clippy passed after the final CJK query font regression. Cross-compilation is not
Windows execution.

The collector passed 77 of 81 tests with four platform/tool skips. Its new native
PowerShell predicate test is skipped locally because PowerShell is unavailable;
Windows CI must execute it. Bundle tests passed 28 of 29 with one platform skip,
export tests passed two, capability tests five, Git fixture tests six, frozen Maven
cache tests eleven, Maven acceptance tests five of seven with two skips, process
resource observer tests 79 and GC collector tests 36.

Independent protocol/security, UI/session and native receipt reviews found no
remaining blocker. Reviews identified and corrected global Escape consumption,
stale line-jump selection replacement and incomplete frontend receipt evidence.
The final helper binds the exact verified native query/result and checks selection
after editor frames, plus actual Undo/Redo text and version changes.

The default-feature release frontend and agent built successfully. Actual Linux
normal-agent capability/trust checks passed with 24 capabilities, and the real
agent-to-mock-LSP process chain passed its explicit literal workspace queries,
no-document operation, empty-query rejection and unchanged-source checks. This
is protocol integration evidence, not a real JDT index witness.

Fresh exact native CI must establish the unopened JDT type and all package gates.
No 0.24 native acceptance is claimed before that run. Windows GUI and authenticated
SSH remain separately unverified. Existing JDT spontaneous-diagnostic loss remains
an openly documented limitation; explicit refresh is a supported mitigation, not
an upstream causal fix.
