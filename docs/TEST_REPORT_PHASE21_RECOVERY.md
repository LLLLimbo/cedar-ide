# Verification report · Explicit Java recovery acceptance / 0.21.1

This follow-up deliberately distinguishes spontaneous JDT diagnostic publication
from the already-supported user-triggered refresh workflow. It does not change
production Java scheduling or add automatic background validation. The preceding
[0.21.0 report](TEST_REPORT_PHASE21.md) retains the exact red CI result; its new
javac navigation passed both native platforms, but its bundle was held.

## Release criterion and retained limitation

The rapid edits, original 60-second spontaneous-push wait and its complete
receipt remain unchanged. A timeout remains a timeout. Only that outcome may
invoke exactly one fixed, user-equivalent refresh of the same synchronized
document/version. Successful recovery requires the exact generated source
witness within the remaining bounded test envelope. No edit is replayed, no
save is sent, and no retry loop is introduced. The normal source, editor Undo/
Redo, process identity and cleanup criteria remain required. Recovery failure
blocks release.

Recovery admission reserves 165 seconds inside the unchanged 360-second fixture
deadline: the existing 75-second refresh request bound, a 15-second event-poll
dispatch window, and one final in-flight 75-second poll. This is not a promise
of a 15-second wall wait or a guaranteed cleanup reserve. Insufficient remaining
budget refuses recovery. The previous failure-only hover probe is replaced by
this explicit recovery; its historical 0.21.0 evidence remains in that report.

The required workflow may pass with a separately recorded spontaneous timeout
and successful explicit recovery. Such a result must be reported as recovered,
never as a spontaneous pass or an upstream fix. The fixture-only permission for
the existing typed URI operation requires its vetted nonshipping Java session;
normal generic sessions stay unsupported, including all-features shipping builds.

The pinned public JDT lifecycle source permits a cancellation ordering that
clears pending reconciliation work before returning on cancellation. The
inspected current upstream handler has the same body. This supports an upstream
work-loss hypothesis; it does not prove the native failure took that path.
Official explicit validation enqueues diagnostic work separately.
See [the pinned handler](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/BaseDocumentLifeCycleHandler.java#L207).

## Verification status

The local aggregate passed 877 Rust tests across 40 suites, with 23 explicit
opt-in tests retained for native/process stages. All six new recovery policy
test groups and the new fixture-authorization regressions passed. Strict host
and MSVC all-target/all-feature Clippy, formatting and diff checks passed.
The collector ran 78 tests: 75 passed and three were skipped locally. Its new
test extracts the actual PowerShell release predicate and exercises positive
and 43 negative receipt cases; that test requires PowerShell and therefore
awaits native CI here. Bundle tests passed 28 with one skip, and export tests two.
Independent source review found no blocking issue.
The normal default-feature release app/agent build and four actual-agent
protocol, capability, language and task smoke suites also passed.

No new native result or ZIP is claimed yet. The existing user-visible stale/
unversioned diagnostic notices and explicit Refresh action remain important;
this change does not guarantee spontaneous diagnostics or general freshness
for unversioned server events.
