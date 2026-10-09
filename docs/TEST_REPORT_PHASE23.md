# Verification report · explicit test-result snapshots / 0.23.0

This checkpoint adds a read-only Tests panel for one explicitly selected relative
report path. It reuses ordinary workspace Read and requires no execution trust,
new protocol operation, compiler, test engine, language server or background scan.
The supported subset and user-visible limitations are in [Test results](TEST_RESULTS.md).

The previous 0.22 checkpoint passed exact Ubuntu/Windows CI at public commit
`39ac129041a422853f40ac30f5f8e450539b9675`,
[run 37890453917](https://github.com/LLLLimbo/cedar-ide/actions/runs/37890453917).
Its real Maven present/missing pair passed in 19,596 ms with one model query each,
required project-marker and offline-POM witnesses false/true, preserved inputs and
verified cleanup. The extracted Windows package passed all checks including the
new Maven trust rejection. Earlier red checkpoints and their distinctions remain
in the [phase 22 final report](TEST_REPORT_PHASE22_FINAL.md) and its linked history.

## Finite acceptance

- Bounded single-testsuite XML parsing; explicit ordinary outcomes and visible
  unsupported retry/flaky cases; no partial-success report after malformed XML,
  limit overflow or inconsistent declared counts.
- No DTD/custom entity resolution, external schema fetch, retained properties or
  system output. Failure text is inert and bounded.
- Exact connection/load/path/revision binding; stale replies cannot replace a
  newer snapshot. Clear/path changes/reconnect invalidate pending view ownership.
- Existing dirty buffers, Undo and recovery remain independent. No inferred source
  navigation, report persistence, test discovery or automatic execution.
- Explicit ignored acceptance drives real normal-agent trust-off reads on both
  platforms, checks the exact upstream fixture identity, newer-load/session races,
  errors, unchanged generated files/draft and reaped process cleanup.

A retained 869-byte Apache Surefire 3.5.4 upstream report fixture has independently
verified source commit, Git blob, SHA256, license and checkout byte preservation.
This is compatibility with that upstream regression fixture, not a new Surefire
or JUnit engine execution. No dependency cache or executable download is added.
quick-xml was already locked transitively; the frontend adds a direct dependency
on the same version without upgrading external packages.

## Current verification status

The final host aggregate passed 951 Rust tests across 40 suites, with 24 explicit
opt-in tests ignored. All 29 new parser/UI tests passed. Strict host and MSVC
all-target/all-feature Clippy, formatting and diff checks passed. The default-feature
release frontend and agent built successfully. The actual Linux normal-agent report
test then passed with seven explicit reads: every fixed trust-off, revision, stale
load/session, malformed/missing, draft/source preservation and joined cleanup
witness was true. It did not start a test engine or language server.

The bundle suite passed 28 of 29 with one platform skip; export tests passed two.
Existing Maven receipt tests passed five of seven (native Windows/PowerShell skipped
locally), and the collector passed 75 of 78 with three platform/tool skips.
Independent parser/security and UI/session reviews found no remaining blocker.
The XML review caught adjacent-attribute and duplicate-BOM acceptance gaps; both
were corrected and covered by passing regressions before these final checks.

Fresh exact dual-platform CI must still execute the new normal-agent read test on
Windows and regenerate/verify the package. No 0.23 native package, manual GUI or
real SSH validation is claimed here.
