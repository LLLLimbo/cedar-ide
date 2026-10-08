# Verification report · Java Organize Imports / 0.20.0 · 2026-10-08

This checkpoint adds one typed, explicit active-document Java import action.
It reuses the existing source-snapshot, preview and atomic Undo pipeline.
The shipping route is limited to the isolated Windows agent’s typed Java
session; generic Linux/macOS LSP sessions do not advertise it.
There is no generic execute-command route, folder/project operation, resource
edit, server-side apply or automatic Save. See [the feature contract](JAVA_IMPORTS.md).

The preceding tab-focus follow-up is preserved in the
[0.19.1 report](TEST_REPORT_HOTFIX_0_19_1.md). Its corrected actual Linux test
passed; its [exact native CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37833947592)
and regenerated bundle are now verified. Its production Java Stop reported
verified forced cleanup in that run, not natural exit. The earlier intermittent JDT diagnostic-publication cause
remains unresolved.

## Boundaries

Trust, typed Java ownership, vetted Standard JDT identity, exact advertised
command, open document and acknowledged version are checked before dispatch.
The request contains the agent-generated current-document URI. Server-provided
support metadata cannot authorize the bridge.

Only the audited changes envelope with zero or one current-document plain edit
array is accepted. Other documents, invalid URI identities, annotations,
commands, document/resource operations and unknown edit fields are rejected.
The frontend checks exact source identity and strict UTF-16 ranges, overlap and
size bounds before Apply. Stale typing/navigation/session changes cannot apply
an older proposal. No-op results leave edit version and Undo history unchanged.

## Finite acceptance

Mock-process tests must prove the exact command and URI, capability/trust/Java/
version gates, strict result validation, and continued rejection of server
workspace/applyEdit. Frontend tests cover synchronization, unsupported sessions,
late responses, Cancel/repeated Apply, source edits, Unicode/CRLF, no-op history
and one complete Undo/Redo transaction.

The normal Windows Quick production fixture extends the existing JVM session
only after its original semantic, editor, refresh and async-start witnesses.
Generated import fixtures must prove unsaved unique-type additions, sorting and
unused removal without disk writes, plus ambiguity without silently choosing
between two known project types. All original cleanup and source invariants
remain required. Resource-baseline and one-time GC workloads remain unchanged.
Only bounded typed evidence is public; raw Java/protocol logs remain private.

## Verification status

The final local aggregate passed 843 Rust tests across 40 suites, with 22 opt-in
cases retained for explicit process/native CI stages. Strict host and MSVC
all-target/all-feature Clippy, formatting and diff checks passed. The normal
default-feature release app/agent build passed, followed by four actual-agent
protocol/capability/language/task smoke suites. Independent source review found
no remaining blocker.

Python checks passed: the extended collector 73 with two platform skips, export
two, bundle 28 with one platform skip, Git fixtures six, process observer 79 and
GC collector 36. The package includes the new imports guide in its explicit
source-file inventory.

Initial actual-frame UI tests exposed first-window layout and covered-tab
coordinates in the tests. The final tests wait for a real paint and move the
actual preview window before clicking document tabs; their assertions remain
intact and production formatting behavior was not changed to fit coordinates.

The new real-JDT Quick acceptance has compiled for Windows but has not yet run
at this checkpoint. Exact native Windows CI and regenerated bundle byte
verification are required; cross-compilation is not runtime evidence. GUI
Trust-on, native Windows/macOS GUI and authenticated SSH remain separate gaps.
