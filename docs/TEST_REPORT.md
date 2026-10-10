# Verification report · Bounded Git capture diagnostics / 0.44.1

The [0.44 failed CI result](TEST_REPORT_PHASE440_RECOVERY.md#exact-source-ci-result) remains preserved. This increment adds only test diagnostics and a controlled Windows pipe-writer lifetime regression. Production capture behavior, output limits and deadlines are unchanged. Recovery-unavailable lifecycle acceptance still requires full exact-source CI and all three package audits.

## Evidence and diagnostic scope

The old Windows error combines a true 256 KiB stream overflow with incomplete stdout or stderr EOF after a 250 ms drain. Its shared fixture assertion identifies neither the repository phase nor the Git subcommand. The old receipt therefore cannot establish either cause.

Test-only diagnostics now record a fixed command category and fixture phase, bounded retained byte counts, the actual cap-hit flag, both EOF flags, drain expiry and whether the existing pre-drain process poll observed root exit. Owner errors remain distinguishable. The snapshot precedes cancellation, and emission follows owner cleanup. No raw command arguments, paths, stream output or error messages enter the record. Three pure Windows tests cover classification, cap/EOF/error combinations, fixed scalar serialization, byte bounds and suppression on success.

A source-backed candidate is an additional child retaining a pipe writer beyond the original job's exit. The process primitive documents that its explicit handle list restricts its own child; unrelated broad-inheritance spawns require host coordination. Parallel fixtures also use ordinary Rust process spawning. This is a candidate mechanism, not a finding that the historical failure used it. The [exact CI Rust source](https://github.com/rust-lang/rust/blob/b940084d7eb6a299eb4bfeb8e34901bc051e7ac4/library/std/src/sys/process/windows.rs) documents the separate standard-library spawn lock and inheritable-handle window.

## Restricted lifetime regression

The new Windows regression creates two independently owned suspended copies of its test executable. Both receive only the same three generated stdio handles through the existing explicit handle and job lists; neither child resumes. After the parent writers close and the first process exits with an empty job, capture must still lack EOF while the second child holds the writers. Releasing the second owner must produce both real EOF flags, zero bytes and joined capture cleanup.

This model does not reproduce broad-inheritance spawning and cannot prove the earlier CI cause. It observes process exit, job emptiness and EOF within one shared five-second budget. Existing OS termination/wait and pending-I/O joining remain potentially blocking on exceptional kernel failures. Assertions follow best-effort settlement; RAII preserves ownership on errors. The existing aggregate CI step has a 15-minute outer timeout. No global spawn mutex, detached owner, security change or production deadline extension was added.

## Verification boundary

Independent reviews cleared the restricted handle-list model, cleanup ordering and sanitized diagnostic schema. Strict workspace Clippy passed on the host and Windows MSVC target. Host all-targets/all-features workspace tests passed: 1,519 passed, zero failed and 56 ignored across 47 suites; Windows-only tests require actual native CI and are not claimed as locally executed. Fresh 0.44.1 release binaries, full retained native gates and packages remain CI requirements. Local release/process results from 0.44 retain their original source identity. The new diagnostics do not turn the old failed run into a pass or establish a capture fix.

The recovery Store, Windows ACL policy, dependency versions and the completed one-shot descriptor investigation are unchanged. No new descriptor probe is scheduled.
