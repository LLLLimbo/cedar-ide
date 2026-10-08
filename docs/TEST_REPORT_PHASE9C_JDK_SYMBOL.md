# Verification report · checkpoint 9C JDK symbol coverage / 0.8.12 · 2026-10-08

This checkpoint adds a standard read-only JDK workspace-symbol assertion to the
direct Java acceptance example. It does not weaken shutdown or enable normal
Windows IDE language capability.

## Verified public baseline

Exact public 0.8.11 commit
[`9d619568d6aab85286d87034c1ef86f3c47b38cd`](https://github.com/LLLLimbo/cedar-ide/commit/9d619568d6aab85286d87034c1ef86f3c47b38cd)
[passed Ubuntu CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37713476684).
Windows passed 21 process cases, 16 transport cases, seven isolated-agent
task/bundle cases, and all three previously skipped agent-language cases. Those
three prove explicit trust/default-gate behavior, independent overlapping task
and language owners, and peer-EOF/forced-agent cleanup of both owned jobs.

Real JDT completed initial semantics, but its shutdown still exhausted grace:
10,115 ms, terminal category grace_expired, retained root exit1067 and graceful
exit false. Restarts were not accepted. That exact commit remains red on the
strict real-Java gate.

## Added assertion

The example checks advertised workspaceSymbolProvider support, then sends the
standard workspace/symbol request for java.lang.String after its existing source
semantic checks. It requires a Class symbol named String in container java.lang,
a nonempty jdt://contents/ URI prefix without whitespace/control characters,
and an ordered range bounded to LSP unsigned31-bit coordinates. It then checks unchanged
source bytes again before document close and shutdown.

For this direct example, JDT's documented classFileContentsSupport initialization
option is enabled and search scope is all, allowing JDK-library symbol results.
This is a symbol-validation profile; it does not claim a class-file viewer or
change normal client capability advertisements. Completion documentation and lazy
import resolution remain enabled. Maven/Gradle import remains disabled for the
generated Eclipse fixture, and no server command is executed.

The symbol result witnesses the requested indexed JDK type. It is not a formal
join of every internal server job, and no universal graceful-exit guarantee is
inferred. All three sessions still require the existing semantic checks, unchanged
source, intended data-directory witnesses and independently observed Windows
root exit0 within the original ten-second grace. New evidence fields are a fixed
boolean/check enum; raw symbol payloads are not added to public artifacts.

## Verification boundary

Local aggregate verification passed 560 Rust tests, all five Python smoke
chains, two export tests and 39 sanitizer tests (two Windows-only skips).
MSVC all-target/all-feature clippy and optimized workspace build passed.
Independent static review found no remaining blocker. The next exact native
CI must establish the new symbol assertion and full Java lifecycle outcome.
GUI Trust, authenticated SSH and actual IDE/headless Java edit acceptance remain
separate pending boundaries. No private diagnostic findings are included here.

Sources: [JDT LS 1.61 workspace-symbol handling](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/v1.61.0/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/WorkspaceSymbolHandler.java),
[JDT client capability checks](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/v1.61.0/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/preferences/ClientPreferences.java).
