# Verification report · explicit Git changes / 0.15.0 · 2026-10-08

This checkpoint adds a trusted, explicitly refreshed Git status and selected-file
diff view. It retains the ordinary file, Java, task and interrupted-save behavior.
The new feature has no staging, commit, reset, fetch or patch-apply operation and
no automatic polling. Local verification passed; exact native acceptance
and the regenerated Windows bundle are pending.

## Verified development bundle baseline

Exact public 0.14.1 commit
[`3558053e14716bd39ca1f3f0968b1cc78c54a10c`](https://github.com/LLLLimbo/cedar-ide/commit/3558053e14716bd39ca1f3f0968b1cc78c54a10c)
[passed both CI jobs](https://github.com/LLLLimbo/cedar-ide/actions/runs/37765260416).
The corrected unsigned Windows ZIP contains the VC++ v14 x64/UCRT prerequisite,
Microsoft links, normal default-feature binaries and complete license/hash data.
Its 524 payload hashes and source/run mapping were verified, as was the actual
Unicode-path extracted trust-off file probe. The ZIP was 4,564,392 bytes with SHA256
`da6544889d11444291a1e2f0d4bc43e60f352a56229083649eda7a86781228fd`.
The preceding report is retained in [phase14](TEST_REPORT_PHASE14.md).

## New boundaries

The [Git view](GIT_VIEWS.md) requires explicit execution trust and a selected Git
2.45+ executable. Windows uses its isolated agent and a separate owned Job;
Unix uses the existing bounded process-group route with raw bytes preserved for
strict status parsing. Every Git subprocess receives a private sanitized child
environment. Existing process-launch constructors still inherit their environment
unchanged, and no global environment mutation is introduced.

The native Windows environment block uses ordinal case-insensitive name ordering,
rejects duplicates/malformed names and retains literal UTF-16 values. Git scrubbing
uses the same native case semantics to prevent Unicode environment aliases from
surviving ASCII-only checks. Tests cover literal child values, replacement versus
inheritance, parent environment stability and owned cleanup.

Git requests have a shared deadline, bounded streams/entry count/metadata scan,
explicit root/worktree, literal paths and no lazy fetch or optional index refresh.
Only ordinary repository roots are supported. External diff, textconv, pager and
fsmonitor are disabled. Repository clean/process filters still execute with the
account's authority: read-view commands are not a sandbox against helper writes
or networking. Global/system configuration suppression is explicit.

The UI validates connection/request/program/selection lineage before accepting
status or diffs. Results remain outside document save baselines, undo/recovery
state and user source. Old-agent legacy status remains available. Native GUI and
authenticated SSH validation remain separate open work.

## Local verification and pending native acceptance

Rust 1.99.0 passed 691 aggregate tests plus 19 explicitly executed process cases
(710 total), formatting, strict whole-workspace host/MSVC checks, default-feature
shipping MSVC checks and the optimized normal app/agent build. The focused app
suite passed 335 tests, including 20 Git state/headless pointer cases; workspace
checks passed 94. Five existing real-agent Python chains passed after fixing a
helper parameter-name collision with the new diff-kind field.

The final normal release-agent/real-Git Linux probe passed 1,276 assertions with
16 typed entries. It verified exact views/readback, unchanged ordinary repository
snapshots, hostile environment isolation, invalid UTF-8 and capture bounds,
retained helper cleanup and independent task survival/cancel. A missing generated
promisor blob stayed absent despite an available local backing repository; diff
failed honestly and both repository snapshots stayed unchanged. This tests the
combined no-lazy-fetch/disabled-transport policy without external hosts.

Python package regressions passed 28 with one Windows-only skip; export passed
two; crash collection passed 69 with two skips; resource sampling passed 79 and
GC collection 36. Independent source review is clear after native Unicode-name
scrubbing and Windows device/path-boundary corrections. The new per-child
environment code adds seven Windows unit and four native lifecycle cases.

Native CI must
execute the new environment lifecycle cases and actual normal-agent/real-Git
flows, including Unicode/literal paths, unchanged ordinary repository snapshots,
redirect/trace suppression, timeout/flood/descendant cleanup and independent task
ownership. The Windows Git probe runs after the default-feature shipping rebuild.
The regenerated development ZIP also carries the Git guide and retains the
runtime prerequisite, unsigned status and existing file-route acceptance.
