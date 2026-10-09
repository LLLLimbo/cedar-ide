# Verification report · Maven dependency provenance / 0.29.0

This checkpoint adds one optional, typed dependency-insight operation for an
already-running trusted Maven leaf session. It separates captured direct POM
declarations, actual JDT library observations and regular-file presence. It does
not infer dependency edges, transitive coordinates, resolution or JAR integrity.

## Required boundaries

The existing Maven capability pair and model reply remain unchanged. The new
operation binds the verified owned startup ID and captured POM SHA-256, checks
the disk POM before/after one fixed bounded settings query, and exposes only
validated workspace/cache-relative paths. Unknown observation is distinct from
an available empty list. Case-colliding declarations retain all bounded matches.
Frontend context also binds generation, session, request sequence and new disk
POM evidence; unsaved drafts and Undo remain separate and intact.

## Finite verification

Portable tests must cover schema/provenance, explicit/default declarations,
relations and collisions, invalid metadata and bounds, unavailable observations,
stale startup/hash/session/generation, capability refusal and frontend rendering.
The existing normal shipping Windows Maven present/missing pair must establish
exact declaration/path/presence and observed-or-omitted evidence, actual frontend
binding and the unchanged source/cache/cleanup conditions within existing outer
budgets. No new dependency downloads or test artifacts are needed.

Independent protocol/provenance and frontend/state reviews have no remaining
findings. Review closed an inherited acknowledgement gap: accepted POM reads in
disk comparison/reload/merge verification, interrupted-save checking and report
loading now invalidate old Maven evidence after their own request/path/revision
checks. Other accepted file reads use the existing Open acknowledgement path.
Rejected or pre-startup reads cannot supply a new witness; drafts remain intact.

Strict host and Windows MSVC-target Clippy pass. The all-target host aggregate
completed 41 suites with 1,134 passed, zero failed and 29 explicit opt-ins ignored.
All 20 focused dependency frontend cases pass, including 256-row reachability,
full reversed selection/focus/Undo and same-frame disk-POM invalidation. Both
all-feature and default shipping release builds pass. The actual default Linux
agent capability/trust smoke passes with the unchanged 24-capability inventory
and no tool startup.

Collector/Maven Python tests ran 99 cases: 91 passed and eight explicit platform
or PowerShell skips. Packaging/capability tests ran 41: 40 passed and one skip.
Actual Windows PowerShell predicates, normal-agent Maven present/missing insight
receipts, and all exact-source package hashes remain required. No 0.29 native
result is claimed. [0.28.1](TEST_REPORT_PHASE281_BUNDLE_GUIDE.md) is the latest fully verified
distribution. Its ordinary Java session 2 spontaneous timeout followed by one
successful unversioned refresh remains distinct from a spontaneous pass; the
underlying diagnostic-publication limitation is not claimed fixed. Windows GUI
and authenticated SSH remain unverified.

## Exact 0.29.0 outcome

Public `9dab1a43615a5dcace270ae70b418d888b7f8863` ran [CI 37950421390](https://github.com/LLLLimbo/cedar-ide/actions/runs/37950421390). Ubuntu passed. Windows failed the aggregate capability test: the explicit expected set correctly contained 31 capabilities, but a separate count-only assertion still expected 30. Windows release, native Maven dependency acceptance and packaging did not run. This remains a failed checkpoint with no new native dependency or ZIP result.
