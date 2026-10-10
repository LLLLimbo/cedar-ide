# Verification report · Recovery-unavailable lifecycle / 0.44.0

The latest accepted checkpoint is [0.43.1](TEST_REPORT_PHASE431_DESCRIPTOR.md). This increment lets the editor handle unavailable recovery storage without losing unsaved text or falsely certifying a recovery copy. Full exact-source CI and all three package audits remain required before this checkpoint is accepted.

## Changed behavior and safety boundaries

Typed actor availability and operation effects distinguish never-invoked requests from acknowledged or possibly applied writes and removals. Older copy evidence survives rejected newer submissions. Retry cannot adopt an unreviewed record, replay a canceled removal, or reopen storage after a canceled retry without a fresh explicit request.

The separate retained-copy quit path freezes recovery admission, cancels queued removals and drains accepted writes plus any in-flight operation. Final confirmation requires an actual matching settlement and unchanged document/workspace/session/profile state. Each UI observation has a fixed five-second bound; expiry refuses quit rather than interrupting storage or claiming settlement. Keep editing invalidates the close and resumes admission only after settlement. The warning states that current unsaved text may be lost and remaining copies may be older. See [recovery behavior and platform limits](RECOVERY.md).

Independent lifecycle and recovery review covered shutdown drain, remove errors after possible unlink, stale acknowledgements, mixed known/unknown copies, canceled Retry/read state, newer edits, reopened document ownership and the language-Stop confirmation bridge. The shipping Store implementation, storage schema and non-Cedar locked dependency data are unchanged. This slice adds no Windows ACL enforcement or privacy guarantee.

## Local verification

The restored official stable toolchain resolved to Rust 1.99.0 (2026-10-01 distribution). Host verification ran in the cloud Linux executor; Windows-target compilation is not native Windows execution.

- Strict workspace Clippy for all targets and features passed on the host and the Windows MSVC target.
- Workspace tests passed: 1,519 passed, zero failed, 56 ignored across 47 completed suites. Required opt-in process checks are accounted for separately.
- Default release workspace compilation passed. The normal-agent acceptance below used the built release agent, not a mock peer.
- All 491 Python cases completed: 482 passed and nine existing platform/tool skips. Formatting and whitespace checks passed.
- The normal-agent unavailable-recovery acceptance passed two generated cases: a regular file at the configured store path and a child beneath a regular file. Each case exercised one explicit Save and a two-file Save All, exact conditional revisions and source bytes, full selections and Undo/Redo, 128 recovery-refresh checks, and owner cleanup.

The acceptance ledger totals two connections, 20 requests, six Writes, four Reads, eight Lists and two reaped agents. Each case writes its files `[2, 1]` times. There are no Run or language operations, no invented recovery-copy/protection claims, no queued recovery mutations and no acknowledged recovery removals. The first local fixture attempt incorrectly asserted that asynchronous Refresh had already completed; that failed receipt is retained. The corrected fixture waits for the matching response within the existing shared five-second cohort deadline while continuously checking unchanged editor and recovery state. No production timing or acceptance criterion was relaxed.

## Required native CI and limits

Both OS jobs retain the existing build, ownership, save, Java, Maven, Git and package gates, and add the same two-case normal shipping-agent acceptance with agent-hash preservation. Local headless and process tests do not establish native GUI behavior.

The [completed Windows descriptor investigation](TEST_REPORT_PHASE431_DESCRIPTOR.md#exact-source-ci-result) is not repeated in routine CI. Its pure parser and driver tests remain. Its inherited-owner/DACL findings and earlier failed receipt remain historical evidence requiring a separate policy decision; no additional native descriptor observation, ACL mutation or recovery plaintext probe is part of this increment.
