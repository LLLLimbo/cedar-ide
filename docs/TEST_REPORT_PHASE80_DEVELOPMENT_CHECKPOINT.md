# Phase 80: 0.80.0 development checkpoint candidate

Status at source sealing: **DEVELOPMENT CANDIDATE; final 0.80.0 validation pending**.
This is a version/documentation consolidation, without a main merge, tag or
formal release. The direct parent is
[`8fa34ae8c5966c9c684ddd96037b17e4c4d69ce4`](https://github.com/LLLLimbo/cedar-ide/commit/8fa34ae8c5966c9c684ddd96037b17e4c4d69ce4),
source tree `2d72c294d5b84a70019f96b1eefecdc66c9433ff`, still version 0.79.0.
It follows the earlier version checkpoint
[`72a486ba0a99a11e55835dd896fee49a82330dae`](https://github.com/LLLLimbo/cedar-ide/commit/72a486ba0a99a11e55835dd896fee49a82330dae).

## Inherited work consolidated here

### Flat navigation and loaded Project names

- In Flat mode, unmodified Home/End on a current focused row selects and reveals
  the first/last accepted row. Navigation clamps rather than wrapping; movement
  does not open a file or send a request. Existing Up/Down and ownership guards
  remain in place.
- Project **Search names…**, or an admitted printable row-origin input, searches
  a case-sensitive literal substring of current accepted Flat names or expanded,
  accepted Tree rows, including rows outside the viewport. It preserves list
  order and highlights matching substrings rather than removing nonmatches.
  Up/Down moves among matches; Escape exits; a fresh independent Enter activates
  the exact selected file/directory through existing behavior.
- This is **loaded rows only**. It does not recursively scan collapsed/unloaded
  directories, discover hidden descendants, run a search backend or automatically
  open a result. It is distinct from Project files discovery, whose existing
  behavior is not changed by this checkpoint. No new filename/path case-fold
  option or full IntelliJ Speed Search equivalence is claimed.

See [Explorer](EXPLORER_TREE.md). Synthetic native-input and lifecycle tests do
not establish comprehensive operating-system IME acceptance.

### Shared Match case for Project Text

Find in Files/Project Text and bottom Find Results share a **Match case** draft.
It defaults to checked and survives in-process surface/provider/hide/reconnect
changes without persistence across restart. Checked root/no-mask requests keep
legacy Search; checked filtered requests keep SearchIn. Unchecked requests use
only the independent negotiated case_search v1 / SearchCase route for all scopes.
An unsupported peer receives an explanation, without a silent fallback, broadening,
new trust requirement or automatic submission.

Unchecked matching is **literal Unicode 16.0 simple case folding**. It is not
normalization, full multi-character folding, regex or whole-word matching. Paths
and basename masks stay case-sensitive; current-file Find and Project-name/file
providers keep their own existing semantics. Original snippets, source bytes and
byte offsets are retained. Toggling, including toggle-away/back, retires old
results, preview and search-origin Open authority without backend cancellation;
physical jobs drain normally, safe inactive reads and unrelated generic Open
behavior remain intact. Pointer/Space/Enter on the checkbox is local; submitting
requires a fresh gesture. Historical counts describe the accepted request mode.

The [backend contract](CASE_SEARCH_BACKEND_STAGE2.md),
[UI contract](CASE_SEARCH_UI_STAGE3_R2.md) and
[independent Stage 4 proof](CASE_SEARCH_ACCEPTANCE_STAGE4.md) retain their exact
scope. The latter has **12 agent operations and 16 witnesses**, separate from the
unchanged scoped proof's **10 operations and 12 witnesses**. They are not OS
file-open counts or a native asynchronous-scheduler race proof. The inherited
maximum-literal TimeLimit contract correction and its original failed observation
remain recorded; no limit, predicate or proof count changes in this checkpoint.

### Kelvin glyph rendering

The existing embedded Inter face is appended after the complete previous
Monospace fallback chain, before an optional installed system CJK face. It renders
the original U+212A Kelvin sign instead of tofu without replacing it with ASCII K,
adding font bytes, downloading fonts or changing matching. Glyphs already
supported by the original embedded/default chain retain that priority; a character
previously supplied only by a system CJK font may now select Inter if covered.
Fallback glyphs use Inter's own advance, so uniform Unicode cell width is not
promised. The font assets, pinned hashes and licenses remain unchanged.

## Exact version/documentation-only scope

This checkpoint changes Cargo.toml's workspace version and only the ten local
Cedar package-version entries in Cargo.lock. Five Rust process proofs, five
Python driver constants and four Python tests update necessary exact-version
literals, including the current-version-plus-suffix malformed inputs. All their
logic, failure rules and budgets remain byte-identical after that literal
substitution. The synthetic 0.79.0 compatibility peer and frozen accepted_079
reader/provenance remain unchanged. Historical phase reports retain their old
versions and evidence.

Current entry-point/product documentation is consolidated here. Runtime code,
protocol/matcher behavior, dependency versions/checksums, font bytes, vendored
code, helper logic, workflow/jobs/permissions, package format, execution trust
and cleanup contracts do not change. No old behavioral assertion is relaxed.

## Verified direct-parent evidence, still 0.79.0

[CI193, run 39190113556](https://github.com/LLLLimbo/cedar-ide/actions/runs/39190113556)
passed its four Ubuntu/Windows aggregate/package and real-JDT signature jobs.
The five Kelvin regressions executed on both platforms. Each canonical normal-agent
case proof ran one test with the exact 12-operation/16-witness receipt; the old
scoped contract remained separate. Parent local validation recorded 2,641 passing
Rust tests and 83 ignored across 53 summaries, plus 31 Python suites with 865
passed and ten skipped. These are parent results, not fresh 0.80.0 counts.

The exact parent Linux desktop artifact 12159981289 was independently audited,
including its source-bound documents, 548 extracted files including the manifest,
binary hashes and modes. A single bounded cloud-Linux native retest used an
observed 1180 x 812 window and lasted 216.138 seconds, ending normally with frontend
exit 0 and no runner termination signal. The original U+212A was visibly readable
as a K-like non-box glyph in Browse results, disk preview, actual target editor
and shared Find Results preview. It was **not** established to look different from
ASCII K. Explicit checked/uncheck/mask searches showed 2/6/3 matching files, with
shared controls and Open/Back preserving the visible dirty draft and selection.

The generated source files and package bytes stayed unchanged. Recovery was
active in that real app. The two observed frontend/agent PIDs no longer existed
after normal exit; this is not global descendant-cleanup proof. Screenshots did
not capture the internal RPC ledger, redo the canonical operation counts, prove
raw selection affinity/full Undo history or cover every scale/window/IME case.
The original 9b84c3ea native tofu observation and intentional two-test red witness
remain retained; the corrected 8fa34ae8 evidence does not erase them. Raw logs and
screenshots are separate evidence, not private payloads included in this source.

Windows CI and package/content validation are distinct from Windows native GUI.
No Windows/macOS GUI, authenticated SSH, broad native OS IME, exhaustive lifecycle
matrix, or performance superiority is established by this parent evidence.

## Final 0.80.0 completion criteria

At this source seal all final-version results below are **PENDING**. Later results
belong to separate receipts bound to the final source tree, exact public commit,
CI run and artifact hashes; sealed source is not rewritten to borrow them.

1. Exact-byte version normalization and structured/raw Cargo.lock checks prove
   the version/documentation-only boundary. Format, strict workspace Clippy,
   full all-target/all-feature Rust checks/builds and all 31 Python suites pass.
   Current normal-agent/proof artifacts are bound to this new source/version;
   no 0.79.0 executable is relabeled or substituted.
2. Independent review clears the frozen source and receipts. The candidate is
   backed up in ordinary commits; its final no-skip commit passes all four
   existing CI jobs and original canonical/package gates without gate changes.
3. New **0.80.0** Windows development and Linux desktop/agent archives identify
   that exact commit and CI run and pass their existing content/manifest checks.
   Windows and Linux audits are recorded separately. Normal default-feature
   frontend/agent pairs stay together; unsigned development limitations remain.
4. The final Linux desktop pair receives one minimal bounded version/native
   smoke with generated files and normal close, followed by file/package
   integrity checks. Core scope is at most five minutes; any necessary extension
   is separately recorded. Earlier 0.79.0 native results cannot satisfy this gate.

Completion is an explicitly limited development checkpoint, not a formal release,
main promotion, tag, full IDEA replacement or benchmark claim.
