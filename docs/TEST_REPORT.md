# Verification checkpoint — 0.54.0 Duplicate line(s)

## Scope and acceptance state

0.53.4 is the latest accepted checkpoint; exact CI, native scope and all preserved failures are recorded in [the selection replacement report](TEST_REPORT_PHASE53_SELECTION_REPLACE.md). 0.54 adds a keyboard-only Duplicate line(s) draft action. Implementation and runtime acceptance remain pending. No menu, remote capability, dependency, filesystem scan or implicit Save is added.

Strict Ctrl/Cmd+D belongs only to the focused interactive editor. A collapsed caret duplicates its LF-delimited logical line; a nonempty selection duplicates all intersected whole lines, excluding a final line whose start equals the upper endpoint. Existing line bytes, including CRLF and mixed styles, are preserved. A terminated block is copied verbatim below itself. For an unterminated final block, a separator precedes the copy: nearest preceding LF/CRLF, default LF. An empty document or trailing empty line duplicates a blank line. Lone CR remains content.

Both Unicode-scalar cursor endpoints move to corresponding offsets in the copy, preserving direction and row affinity. One effective text transaction is one Undo restoring original bytes and complete selection. Input/result are each limited to 1 MiB and the block to 4,096 lines. Invalid endpoints, overflow or saturated edit version refuse without partial mutation. Original input ownership is captured before other handlers can filter events; mixed/repeated/composition/stale/focus/modal input cannot become a clean duplicate action.

The action changes a draft only. Explicit Save remains separate. Ordinary recovery and already enabled language synchronization can observe the edit; zero-total-operation proofs apply only with Trust off and no active language service.

## Required acceptance

Independent newline/history/input review; pure and actual-frame Unicode, newline, selection, bounds and stale-input matrices; both-platform normal-agent exact operation ledgers with unchanged fixture hashes; all retained CI gates and three packages; and a fresh exact-package cloud Linux GUI check of collapsed/multiline LF/CRLF duplication, immediate Undo/Redo and explicit discard are required. Native observations do not establish raw affinity, backend operation counts, Windows GUI or child reaping.

Local Rust compilation, aggregate tests, release builds and process execution are deferred to fresh CI because disk reserve is constrained. Only source review, formatting and Python/package checks run locally. Maintain at least 200 MiB free after projected source/export/native evidence storage; stop before breaching that reserve. No cleanup is authorized. Windows recovery admission integration remains paused; historical Git incomplete-EOF, client EOF-marker and earlier native selection-refusal causes remain unproven.

## Authored proof and local checks

The authored regression coverage contains 22 pure/production-frame tests: 21 in the duplicate-line module and one language-panel test of ordinary synchronization. Its simulated operating-system modifier cases are not native macOS or Windows GUI evidence. A recording worker checks that ordinary already-enabled language synchronization still follows draft edits; the real process proof intentionally runs Trust off without a language service.

The normal-agent proof contains 20 ordered cases and a strict 47-field receipt. Setup uses Hello, root List and two Reads; explicit reconnect adds Hello and root List, for exactly six requests and two owned agents. Duplication actions issue zero requests and retain both original disk hashes. The proof covers LF/CRLF/mixed/EOF/empty lines, scalar/full-selection Undo/Redo, bounds, stale capture through a real reconnect, other tabs, saving and both token-bearing and unverifiable unknown-save metadata. Synthetic save states are not claims that a peer failed in this run.

All Client calls admit their full 30-second bound against one 60-second active deadline. Cleanup has a five-second reserve inside a 65-second total; success requires observing both owners reaped and the watchdog joined before generated-root removal. Uncertainty retains the fixture and fails. The driver keeps 120-second compilation, 15-second exact selection and 90-second runtime bounds, with bounded teardown within a five-minute CI step. Local Rust remains uncompiled/unexecuted; all authored Rust and process verdicts await fresh CI.

Local checks pass formatting/whitespace, 15 strict duplicate-driver tests, 39 Linux desktop package tests, 30 Linux agent package tests, 36 Windows package tests (one host skip), and two public-export tests. The first attempted export-test command used a nonexistent module name and was rejected by unittest discovery; that invocation log is retained, and the existing `test_public_export.py` subsequently passed. No Rust compile or runtime verdict is inferred from these checks. Structured lockfile review confirms only ten Cedar package versions changed, with third-party dependencies unchanged.
