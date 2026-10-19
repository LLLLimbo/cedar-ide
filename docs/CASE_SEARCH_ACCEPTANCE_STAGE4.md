# Match case: independent Stage 4 candidate proof

This is a new acceptance contract for the Stage 3 Match case UI. It does not
replace or alter the existing ten-operation scoped-search proof or its receipt.
The implementation is a candidate until its exact source tree and fresh gate
records have been independently checked. This document does not assert a run.

## Scope

Only a new app test module, its cfg(test) registration, this document, a new Python
driver and validator tests, and two necessary CI steps are added. No production
search, matcher, protocol, dependency, lockfile, version, permissions, secrets,
global CI timeout, or job topology changes are required.

The app proof uses actual raw_input_hook and App::update egui frames. Commands
from a recording Worker are bridged through the production Client to one ordinary
untrusted stdio cedar-agent. This proves that these app routes and the normal
agent compose at the controlled delivery boundary. It does not execute the
shipping asynchronous Worker scheduler, native OS GUI, real SSH, packaging, or
performance/RSS audits. Those remain separate gates. Agent-operation counts are
not operating-system file-open counts: fixture creation, hashing and cleanup
also use the filesystem.

## Fixed fixture and requests

The literal query is `K [x].* café 😀`, limit 500. Six generated UTF-8 CRLF files
have exactly one matching line 2, plus a decoy `K xZZ café 😀 decoy` that must not
match. Line 2 retains its original K/K/k spelling and a source label:

| Path | Header | Variant | Label |
| --- | --- | --- | --- |
| src Ω/a origin.rs | saved | K | origin |
| src Ω/b target.rs | target | K | target |
| src Ω/nested/c note.md | nested | k | nested |
| root.rs | root | K | root |
| outside/d untouched.txt | outside | k | outside |
| src Ω/z masked.RS | masked | K | masked |

Each file is `{header} Ω😀\r\n{variant} [x].* café 😀 {label}\r\nK xZZ café 😀 decoy\r\n`,
with `last\r\n` appended only to origin. The dirty origin changes only header
`saved` to `dirty`; its saved bytes, reversed selection with affinity, full bounded
native Undo/Redo and edit version are observed. Undo/Redo themselves may advance
edit version; subsequent non-edit blocks use a newly captured version baseline.

There are exactly **12 operations**, separately from the **16 ordered witnesses**:

1. Hello
2. List root
3. Read origin
4. checked root Search
5. checked SearchIn, scope `src Ω`, masks `["*.rs"]`
6. unchecked SearchCase, same scope/mask
7. Read target for unopened preview
8. Read target for search-origin Open, later retired before App delivery
9. unchecked root SearchCase, no masks
10. unchecked SearchCase, scope `src Ω/nested`, ordered masks `["*.rs", "*.md"]`
11. unchecked root SearchCase, masks `["*.rs"]`
12. unchecked SearchCase, scope `src Ω`, masks `["*.zz"]`

Totals: Hello 1, List 1, Read 3, Search 1, SearchIn 1, SearchCase 5, Write 0,
Other 0; one connection attempt, one connection, one reaped owned agent. All five
Case requests echo exactly query, scope, ordered masks, limit and false boolean.
The checked routes remain the legacy operations. All short fixture replies must
be exact complete success with empty incomplete reasons; no maximum-input
TimeLimit allowance is imported into this proof.

Returned rows in the seven searches are 2, 1, 2, 6, 1, 3, 0, sorted by path/line,
with exact original snippets. The nested reply is retired and accepts zero new
rows. Historical root rows may remain inactive; a returned row is not an accepted
row. Examined entries for the scoped/Case searches are 5, 5, 9, 1, 9, 5; directories
count but the initial scope does not. Read bytes are exactly the eligible original
file-byte sums, including nonmatching eligible files. The final mask reads zero
bytes. The uppercase `.RS` file proves masks remain case-sensitive.

## Authority and local-only witnesses

The 16 ordered witness names are fixed independently in the Python contract:
trust_off_normal_agent; checked_legacy_root; checked_filtered_route;
shared_native_case_controls_local; unchecked_directory_echo;
local_grouping_and_movement; dirty_preview_and_return; unopened_preview;
toggle_back_retires_search_open; fresh_root_cached_open_exact_back_undo;
nested_reply_retired_before_delivery; root_basename_mask; complete_no_match;
local_edit_retirement; retained_shell_and_provider_state;
exact_ledger_disk_and_owner_cleanup.

Pointer, Space and Enter toggle the native checkbox without submitting. Both
Project Text surfaces share the draft; grouping, movement, filter edits, dirty
preview, provider/surface/hide/reopen transitions, and cached Open/Back/Undo/Redo
must not dispatch extra agent operations. Every production frame is followed by
an empty command-queue check except the seven explicit submits and three Reads.
Trust stays off; no recovery actor, language session or optional font probe starts.

For Open #8, the real File reply is held after Client completion. The bottom Find
entry preserves the pending Open's navigation epoch; full current SearchOpen
identity is checked before the native toggle. Toggle-away/back retires activation
and its matching history ticket irreversibly. The eventual real reply drains the
App job and safely caches target inactive without navigation, jump or history.
The new inactive document has no fabricated native TextEdit state. A fresh root
query then locates target by actual accepted path/line (not its former index), and
opens the cached buffer with zero extra Read, followed by exact Back and native
Undo/Redo. The old ten-operation proof separately retains ordinary successful
unopened Open coverage.

For query #10, the real validated reply is held at the App delivery boundary.
While the App job remains pending, toggles retire authority but do not cancel the
backend, dispatch replacement work, or clear busy until delivery. Delivery drains
without active rows, preview, error overwrite or history revival. This is not a
claim that the backend is still executing while the completed reply is held.

## Bounds, independent validation and failure

The new exact ignored Rust test is
`case_search_process_tests::normal_agent_case_search_is_explicit_local_and_source_bound`.
It requires `CEDAR_CASE_SEARCH_AGENT_BIN`. Its sole success receipt starts
`case_search_acceptance ` and failure-only progress starts `case_search_progress `.
The fixed JSON schema independently validates the ordered cases, ledger, exact
returned rows/counters, hashes, flags, types and cleanup facts. Twelve operations
must not be mistaken for twelve cases.

The driver reuses the existing aggregate all-feature test artifact selection and
ordinary release agent. Compile is bounded at 120 seconds/4 MiB; runtime
observation at 420 seconds/256 KiB. The proof has one shared 340-second active
budget, a 30-second request admission reserve, a shared 30-second cleanup deadline,
and a 390-second cancellation watchdog. Failure never renews these budgets.

Ownership is registered immediately after successful Client spawn/Hello. An
inaccessible failed connection attempt cannot establish cleanup and retains the
fixture. Failure cancels the token, closes/reaps an owned child within remaining
cleanup time, joins the proof-owned watchdog and retains generated files on
uncertainty. Success follows exact ledger and disk hash checks, owned-child reap,
watchdog join and verified generated-fixture removal. Client close establishes
owned-child reap, not transport-reader thread joins. Driver termination/reaping of
the Rust test does not prove agent cleanup. No retry or later success masks a
failed attempt.

Validator tests reject missing/extra/wrong-type fields, boolean coercion, changed
or reordered requests/masks/rows/cases, duplicate and escaped-duplicate JSON keys,
NaN, malformed/truncated UTF-8, missing/duplicate receipts, wrong/zero test counts,
wrong artifacts, changed agent bytes, and bounded stage failures. Windows CRLF
and redirected cp1252 streams are included. Matching agent version and hash alone
is not source-build provenance: the gate record binds source, build command,
selected executable and agent digest separately. Local selected-debug-artifact
runs must be labeled accordingly; canonical CI driver uses the release agent.
