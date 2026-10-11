# Verification report · Loaded Explorer files / 0.49.0 (pending)

Latest accepted checkpoint is [0.48.2 Enter selection handling](TEST_REPORT_PHASE48_ENTER.md#final-verified-acceptance--0482), public `f925d8b471a08ad732a3fa3961ccbd74cf9037bf` and CI 38095477287. Earlier failed runs and the unknown client/Git failure causes remain preserved. Windows recovery admission remains paused.

This pending slice adds an explicit Loaded Explorer files scope to Ctrl/Cmd+P. Current directory remains the default and retains its live listing behavior. Loaded scope freezes only current-session, current-mode admitted Explorer files plus open buffers; filtering and scope changes perform no List, Read, scan or indexing. Flat-mode admission is separate from retained display rows.

The snapshot is bounded to 4,096 cached paths plus 32 open buffers and checked 1 MiB path storage; display stays at 64 matches. Oversized snapshots are visibly refused as a whole. Exact candidate identity, mode and session are checked at activation; a removed/retyped/invalidated row cannot retarget an Enter or click. Accepted stale listings are labeled as snapshots, not current disk truth. An already-open dirty buffer preserves its selection/Undo with no Read; unopened paths use the existing ordinary Open route.

Acceptance remains pending independent cache/input/session review, deterministic regressions, both-platform normal-agent operation ledgers, all prior CI gates and three packages, then bounded exact-package cloud Trust-off keyboard navigation across sibling folders with unchanged generated files. No real SSH compatibility or new privacy guarantee is claimed.

## Focused local validation and evidence limits

Independent cache/input/session review is clear. The focused navigation suite passes 30 tests (16 retained and 14 new); the same compiled harness passes 29 Explorer and 17 workspace-access regressions. Formatting and strict host/MSVC all-targets/all-features Clippy pass. External dependency versions and shipping capability declarations are unchanged; only the ten Cedar versions change in Cargo.lock.

Review found and corrected pointer ownership, native scope-activation text/focus, failed-refresh stale labeling and Ctrl/Cmd+Enter scope-focus issues before sealing. The first focused run had 21 passes and four fixture-focus failures. Its raw log was overwritten and is unavailable; a separately labeled reconstruction describes the backward-Tab settle-frame correction and its provenance limits. Preserved final logs are execution evidence; the reconstruction is not a substitute for the missing raw output.

Disk constraints deliberately limited local compilation to these focused suites and strict checks. No local full-versioned aggregate, release rebuild or new normal-agent process runtime was performed. Fresh CI must run the full workspace, build default releases and execute exactly one loaded-files process acceptance on each OS: ten cases, six explicit setup Lists, two Reads (one setup, one cached-file activation), zero chooser Lists, zero dirty-buffer Reads, zero Writes/other operations, two connections/reaped owners and five unchanged source hashes. The driver also delays delivery of a real explicit List response until after snapshot creation, then checks that it cannot retarget activation. Its shared watchdog is 60 seconds, ordinary Client call limit remains 30 seconds, and per-owner cleanup observation is 5 seconds. The CI subprocess allows 150 seconds including harness compilation under a three-minute step; no runtime deadline is extended.

All three packages and fresh exact-archive cloud GUI acceptance remain required and pending. No older agent binary substitutes for that runtime proof.
