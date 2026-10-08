# Verification report · Explicit Java diagnostics refresh / 0.16.0 · 2026-10-08

This checkpoint adds one explicit Java-only validation notification and honest
active-document diagnostic status. It does not claim to fix the intermittent
JDT correction-publication failure. The last verified downloadable baseline is
[0.15.4, exact dual-platform CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37787319950),
whose actual Git checks passed 1,421 assertions on Windows and 1,283 on Linux,
with unchanged repositories, missing-promisor protection and owned cleanup.
Its six fixture tests and 25 Windows lifecycle cases passed. The verified ZIP
contains 525 payload files and a manifest; its SHA256 is
`29bc1e207553234783075b16d961d939f84d7721728874cd3817789b0e5b8d20`.
The preceding implementation/native history is retained in
[the phase-15 report](TEST_REPORT_PHASE15_NATIVE.md).

## Bounded operation and compatibility

Protocol 4 gains an optional `java_diagnostics_refresh` bridge capability and
`LanguageRefreshJavaDiagnostics { path, version }`. The server requires existing
execution trust, a production typed-Java session, exact known Standard JDT
identity/version, an opened Java source with Java language ID, root-bound URI
and the positive current synchronized version. Generic sessions, unsupported
versions, Syntax mode, closed/non-Java paths and stale versions fail before any
notification. The agent owns and overwrites the initialize support flag.

One fixed `java/validateDocument` notification contains only `textDocument.uri`.
There is no arbitrary method/parameter bridge, source replay, save, reopen,
automatic retry or new poll loop. Its acknowledgement means notification sent;
it cannot prove the server accepted it or produced diagnostics. The supported
initial identity is Standard JDT `1.61.0-SNAPSHOT`, the Maven version reported
by the vetted 1.61 milestone. Other compatible versions need separate qualification.

## UI freshness and failure handling

The active document shows pending, stale, unversioned or matching-version status,
including empty batches. Refresh captures connection, session, document, path,
edit revision and LSP version. New typing, resync, close/reopen and reconnect
invalidate old action responses. Stale connected errors cannot overwrite newer
UI state; transport failures still disconnect. Rejected/lost diagnostic evidence
cannot leave an earlier empty snapshot marked current. The action requires an
already synchronized draft rather than implicitly replaying a change.

JDT 1.61 push diagnostics omit document version. They remain unverified in the
UI even after an explicit request; receipt order alone proves no correlation.
The original rapid apply/undo/redo/correction acceptance remains required and
cannot be rescued by refresh or by the failure-only hover witness.

## Separate real-JDT witness

After the normal production test has passed all original version-2/3/4/5 editor
and diagnostic assertions, the quick profile synchronizes a new version-6 draft
with a unique unused-variable warning, sends the explicit refresh and awaits its
exact synthetic diagnostic witness under the existing deadline. Typed booleans
record support, notification acknowledgement, observed content witness and
whether that batch lacked a version. Source bytes and owned cleanup remain
required. This checks the real extension flow, not whether it caused publication
or repaired the intermittent failure. Long resource observation and GC-control
profiles do not execute this extra phase, preserving their workload.

## Verification status

The integrated local Rust suite passed 721 tests; 22 explicit process/native
cases remained ignored in the aggregate and are required in their separate CI
stages. This includes six backend refresh tests, protocol/client compatibility,
and deterministic/headless UI cases for snapshot binding, stale errors,
malformed replacements, empty/unversioned batches and equivalent known Java
file URIs. Core client/protocol/workspace unit suites separately passed 129.
The final Unicode-control URI guard was followed by a passing affected-app rerun: 356 tests passed, 14 explicit process cases ignored.

Python checks passed: collector 70 with two platform skips, process-tree observer
79, GC collector 36, bundle 28 with one native-only skip, export two and Git
fixture six. Independent read-only review found no remaining blocker. Strict full-workspace
host and MSVC checks passed with warnings denied. The normal default-feature
release app/agent build passed, followed by real-agent protocol, capability and
language-bridge smoke checks. Formatting and diff checks passed. Exact native CI
must still verify the real JDT refresh flow, owned cleanup and updated Windows
bundle before claiming this checkpoint is verified on Windows.
GUI Trust actions and authenticated SSH acceptance remain separately unapproved;
headless synthetic authorization does not grant either.
