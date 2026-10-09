# Verification report · Java implementation locations / 0.28.0

This checkpoint adds an explicit Java implementation-location query for a running
typed, trusted Java session. It is a navigation feature, not a call graph or a
guarantee that every concrete runtime implementation has been found.

## Required boundaries

- The agent and actual server must support the operation. Dispatch requires the
  acknowledged source version and a valid UTF-16 cursor position.
- Only bounded ordinary locations are accepted. Location links, malformed shapes,
  invalid ranges and oversized results are refused; navigation uses the existing
  root-confined URI resolver and ordinary file read.
- Source, participating drafts, cursor, request sequence, connection generation
  and language session remain bound through query, result selection, resolve and
  read. Newer intent, edits, cancellation or session changes invalidate stale work.
- Existing dirty drafts, full selections and Undo/Redo remain intact. Queries do
  not save files, start servers, change trust or request an index rebuild.
- Results have no target-document version and may lag indexing or unsaved edits.
  Type queries can return subtypes; method queries return declarations rather
  than a separate result for each class inheriting that method.

## Finite acceptance

A generated interface, concrete implementation and inheriting subclass establish
exact type and method result URI/range witnesses. Target files begin unopened;
non-BMP text before identifiers distinguishes UTF-16 positions from code points.
An unimplemented interface declaration supplies a separate empty-result case.
The normal shipping agent/Client route must navigate an actual result through
the frontend, preserve dirty-buffer history, leave source bytes unchanged and
complete the existing bounded cleanup checks.

Deterministic tests cover unsupported providers/agents, version mismatches,
malformed or excessive results, forbidden targets and late query/resolve/read
responses after every relevant state change. All existing native Java, Maven,
file, task, ownership and package gates remain required.

## Captured close-click integration

The new same-frame cancellation test exposed an existing tools-close edge case:
egui recognized a captured primary click after the bottom panel moved, while
Cedar's extra current-rectangle check rejected it. The close guard now uses the
recognized primary click with a strict single-click input batch. Mixed input,
keyboard ownership and later focus changes remain guarded. A standalone moving
panel regression accompanies the implementation-navigation race test; existing
sidebar and workspace-access tests remain required.

## Current status

Independent backend/acceptance and frontend/state reviews have no remaining
findings. Strict host and Windows MSVC-target Clippy pass. The host aggregate
completed 40 suites with 1,100 passed, zero failed and 29 opt-ins ignored. Focused
runs passed all 17 implementation/frontend cases, 12 backend/protocol/client
cases, 17 workspace-access cases and 14 unchanged sidebar regressions. Collector
and capability Python tests ran 94 cases with 88 passed and six explicit
platform/PowerShell skips; bundle Python tests passed 28 of 29 with one skip.
Native PowerShell and actual Windows JDT witnesses remain required in CI.

All-feature and default shipping release builds pass. The actual default Linux
agent capability/trust smoke passes on a disposable fixture, reporting the
unchanged 24-capability Unix inventory and starting no tool. No new real-JDT or
native GUI result is claimed.
[0.27 acceptance](TEST_REPORT_PHASE27_WORKSPACE_ACCESS.md) remains the
latest verified checkpoint. Exact-source local checks, native dual-platform CI
and complete package verification are required before this checkpoint is ready.
Windows native GUI, authenticated SSH and full IDEA equivalence remain separate
unmet validation or feature boundaries.

## Exact 0.28.0 outcome

Public `f6be7c9500ecd32a2a2ee21b672b0328f954179e` passed [both OS CI jobs](https://github.com/LLLLimbo/cedar-ide/actions/runs/37940287794). The new normal-agent JDT witness passed in 16,729 ms: two exact subtype locations, one method declaration, no result for the empty case, and all 24 semantic/frontend/source/cleanup flags true. The native collector/PowerShell matrix ran 89 tests: Windows 88 passed/one platform skip, Ubuntu 87 passed/two platform skips.

Final package inspection found `JAVA_IMPLEMENTATIONS.md` missing despite its quickstart link. The existing 533 payload hashes and provenance matched, but distribution completeness failed. That ZIP remains unchanged and is not accepted as a complete release package. A narrow 0.28.1 inventory and link-validation correction requires its own exact CI and ZIP verification.
