# Verification report · checkpoint 9C independent fixtures / 0.8.11 · 2026-10-08

This checkpoint moves independent synthetic Windows agent-language tests before
real Java acceptance. A failed required group still fails the job; no
continue-on-error or acceptance condition is added.

Exact public 0.8.10 commit
[`6c162726b10f46632b6a7833989be082e38b1476`](https://github.com/LLLLimbo/cedar-ide/commit/6c162726b10f46632b6a7833989be082e38b1476)
[passed Ubuntu CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37712484260).
Windows passed all 21 owned-process cases, all 16 language transport cases
including the four new graceful-shutdown regressions, and seven isolated-agent
task/bundle cases. Real JDT completed initial semantics, preserved source bytes
and produced the intended data-directory witness. It then exhausted the existing
shutdown grace: 10,321 ms, terminal category grace_expired, retained root exit
1067, graceful exit false. Later restart sessions and the three synthetic
agent-language cases were skipped. Real Java acceptance remains unresolved.

The three synthetic agent-language cases use the separate nonshipping opt-in
fixture, marked generated roots and explicit run authorization. They test trust
negatives, simultaneous task/language lifetimes, EOF and forced-agent cleanup.
Their outcome does not depend on JDT's orderly exit, so they now run first. Normal
Windows language capability and GUI Trust checks remain unchanged. Authenticated
SSH and GUI Trust approvals remain absent.

Only workflow ordering, checkpoint version and reports change. Local aggregate
verification passed 559 Rust tests, all five Python smoke chains, two export
tests and 39 sanitizer tests (two Windows-only skips). MSVC all-target/all-feature
clippy and optimized workspace build passed. The next exact
native run is required for the three previously skipped agent cases. Real Java
still requires all three semantic sessions and independently observed zero exits;
forced cleanup cannot satisfy it. No private diagnostic material is included.
