# Verification report · Java build locations / 0.21.0 · 2026-10-08

This checkpoint adds explicit bounded javac-location extraction from a completed
command task, followed by workspace-confined file navigation. It retains the
actual submitted command/session identity and current unsaved drafts. It does
not add an execution request, project importer or persistent background process.
See [the feature contract](BUILD_PROBLEMS.md).

The preceding [0.20 report](TEST_REPORT_PHASE20.md) records its exact dual-platform
CI, Organize Imports witnesses and verified bundle. Its normal Java Stop exited
naturally in that run; forced cleanup remains a supported, honestly reported
outcome in other runs. The intermittent upstream diagnostic issue remains open.

## Finite acceptance

Pure parser tests cover the documented English grammar, per-stream positions,
Unicode/spaces/CRLF, backend-specific separators, malformed/overflowing lines,
unsafe paths and bounded work/storage. Headless frontend tests must preserve
immutable task association, dirty drafts and Undo, reject stale task/session/read
results, enforce the buffer limit, and never start a command/language server or
auto-save from extraction/navigation. Ordinary document synchronization to an
already-running language session remains intact. Historical line numbers must
not silently clamp to a current source location.

An opt-in test uses an explicitly selected JDK 21 javac through the normal agent
on generated source. It must verify the completed task, extracted relative
location, normal Read, source invariants and cleanup. The test compiler version
is recorded; this is not Maven/Gradle or arbitrary localized-output coverage.

## Verification status

The final local aggregate passed 869 Rust tests across 40 suites, with 23 opt-in
cases retained for explicit process/native stages. The 26 new parser and actual
frontend-frame tests passed. Strict host and MSVC all-target/all-feature Clippy,
formatting and diff checks passed. Python bundle tests passed 28 with one platform
skip; the two export tests passed. The normal default-feature release app/agent
build and four actual-agent protocol/capability/language/task smoke suites passed.
Independent review found and closed an ASCII
case-variant dirty-buffer ambiguity, with pre-dispatch and response-time tests.
The guard does not establish physical file identity for other aliases.

The local machine has JRE 21 but no javac executable, so the new opt-in compiler
case has compiled but has not run here. CI must execute it explicitly with each
runner's selected JDK 21. Cross-compilation is not native runtime evidence. No
native runtime result or regenerated bundle is claimed for this source yet.
Existing Java, ownership, Git, save and cancellation gates remain required.
Native GUI Trust-on, Windows/macOS GUI and authenticated SSH remain separate gaps.

## Exact 0.21.0 native outcome

Public `dc6a75e980fd5f55376f5924eb1dcea22f5d45aa` ran
[the exact native workflow](https://github.com/LLLLimbo/cedar-ide/actions/runs/37849097799).
Ubuntu passed. The new javac acceptance passed on both platforms using JDK/javac
21.0.12.1: four starts, four reads, and 36/18 polls on Ubuntu/Windows, with all
location, draft/history, stale-response, source and cleanup witnesses true.

The Windows job nevertheless failed the existing fresh-data Java session 2
correction-diagnostics check: the original 60-second wait recorded zero events
and batches. A separate later hover matched the correction-only symbol in 26 ms.
That establishes corrected-source availability by the later query, not the
cause or time of missing diagnostic publication. Source/cleanup checks passed
and the JVM exited naturally. Direct Java, normal production Java, Organize
Imports and real Git passed; bundle generation was skipped. This is a red
checkpoint, not a verified release bundle.
