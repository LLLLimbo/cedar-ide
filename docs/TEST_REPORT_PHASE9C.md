# Verification report · checkpoint 9C / 0.8.4 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

This checkpoint prepares real Windows Java and isolated-agent language acceptance
without enabling ordinary Windows IDE language services. The normal binary,
constructors, Hello capabilities and execution-trust rules remain gated even in
all-features builds. No native GUI Trust-on or authenticated SSH test was run.

## Verified prerequisite

The preceding 0.8.3 public commit
[`0788ac76b8e2217f3c5e8e70e9b3803894b78230`](https://github.com/LLLLimbo/cedar-ide/commit/0788ac76b8e2217f3c5e8e70e9b3803894b78230)
passed [same-commit Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37691525034).
Windows actually executed 48 process-library unit tests, 21 primitive lifecycle
cases, 12 owned language-transport cases, seven isolated-agent/bundle cases and
two stdio integrations. Both earlier fixture failures were corrected and passed;
see [9B](TEST_REPORT_PHASE9B.md) and [the fixture correction](TEST_REPORT_PHASE9B_FIX.md).
This prerequisite does not establish the new 9C Java/agent behavior.

## New direct Java acceptance

The real `java_smoke` example now requires exact initial diagnostics, semantic
hover/completion and a single exact local definition, deferred import resolution,
a correction-specific warning/hover instead of accepting stale empty diagnostics,
unchanged source bytes, and three serialized initial/fresh-data/same-data sessions.
Windows additionally requires an explicit native Java executable, retained process
identity and zero-code graceful shutdown. Java-consumed paths are verified local
drive paths; the executable retains its native Windows path.

A real Linux run exposed JDT's alternate raw/escaped Unicode URI representation;
strict equivalent-local-path comparison corrected it without accepting other
files or weakening range/version checks. The final 0.8.4 candidate rerun passed
all three sessions with lazy imports in 34,909 ms, including fixture removal.
This is a synthetic functional run, not a benchmark or Windows result. Linux
still has its existing direct-child shutdown fallback, with no independent
Windows-style exit-code, Job, descendant or listener claim.

The Windows PowerShell CI script selects an explicit runner Java 21+ installation,
records its actual build, and verifies the pinned official JDT 1.61.0 archive hash
before extraction. It prepares the test parent's environment, records source/Rust/
runner metadata, checks every command, and emits final PASS only after cleanup.
External distributions/notices stay in generated scratch and are not packaged.
The Windows step's eight-minute deadline is a failure boundary, not cleanup proof.
See [Java acceptance](WINDOWS_JAVA_ACCEPTANCE.md) for exact checks and omissions.

## New nonshipping agent fixture

A required-feature `cedar-agent-language-validation` binary uses the actual
`cedar_agent::serve` and Workspace bridge with a marked synthetic root, fixed
IsolatedAgent host and separately explicit `--allow-run`. No flag, wire operation
or capability was added to ordinary cedar-agent. The marker is an opt-in, not a
sandbox. Release artifact paths exclude this fixture.

Three opt-in native cases check normal-host/trust gates, four startup/coexistence
waves with independent stops, and peer EOF/forced owner death with both task and
LSP descendant trees alive. Cross-gated fixture startup proves overlapping work;
exact retained observation handles and exclusive lifetime files verify cleanup.
The agent's sequential initialization limit is unchanged and remains disclosed.
See [agent validation](WINDOWS_AGENT_LANGUAGE_VALIDATION.md).

## Local checks

- Whole-workspace/all-target/all-feature aggregate: **550 passing Rust test
  executions**, zero failures; two export regressions and five Python agent/tool
  chains passed. Six opt-in stdio cases were explicitly executed. Nine initially
  ignored entries remain separately scoped, not treated as aggregate passes.
- Ten direct Java example unit tests passed on Linux. Windows-only path/process
  assertions compiled but have not run locally.
- Strict whole-workspace/all-target/all-feature MSVC cross-clippy passed.
- Linux whole-workspace/all-feature release build passed.
- Normal-feature agent/workspace regressions also passed during fixture review;
  all-features does not opt normal hosts into language execution.
- Independent source reviews checked the direct probe, PowerShell setup, Windows
  path normalization and the nonshipping fixture/trust boundary.

Raw local files: `verification-phase9c-aggregate.txt`,
`verification-phase9c-windows.txt`, `verification-phase9c-release.txt`, and
`verification-phase9c-java-linux.txt`. Machine-specific raw files are omitted
from public source export.

## Native verdict still required

The new Windows CI must actually execute real Java, followed by all three new
agent-language cases, against the final published source. Missing dependencies,
skipped/ignored tests, cross-compilation and Linux Java success are not passes.
Normal Windows IDE LSP remains closed. Real Windows Java through the agent/editor,
independent Job/descendant/listener evidence, broader project behavior, native
GUI and authenticated SSH remain separate acceptance work.
