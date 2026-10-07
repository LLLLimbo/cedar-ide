# Verification report · 7B revision / 0.7.1 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar **0.7.1**, protocol **4**, corrects two Windows lifecycle test assumptions.
There is **no process-launcher, task-supervisor, transport or execution-policy
change** from 0.7.0. CREATE_NO_WINDOW and the isolated-agent restriction remain.
Windows Git, legacy synchronous Run and language services remain unavailable.

## Evidence from the 0.7.0 run

Public commit [`c28c8dd5b19dab8a900077e79349f34854b83e12`](https://github.com/LLLLimbo/cedar-ide/commit/c28c8dd5b19dab8a900077e79349f34854b83e12)
completed [Ubuntu CI successfully](https://github.com/LLLLimbo/cedar-ide/actions/runs/37671366318).
Windows passed workspace tests, release build and the existing two stdio process
tests, then [failed two of thirteen primitive lifecycle tests](https://github.com/LLLLimbo/cedar-ide/actions/runs/37671366318/job/112963650278):

- The running root/branch/leaf tree had four job-associated processes, while the
  test expected exactly three
- The running idle writer had two job-associated processes, while the test
  expected exactly one after capture cancellation

The other eleven lifecycle tests passed. Both exact suspended-root counts passed.
The **seven new agent/bundle tests were not reached**, so 0.7.0 did not establish
runtime acceptance of that integration. The historical [7B report](TEST_REPORT_PHASE7B.md)
records local checks available when it was sealed.

## Correct contract and preserved checks

Microsoft's [console design](https://github.com/microsoft/terminal/blob/main/doc/specs/%23492%20-%20Default%20Terminal/spec.md#inbox-console)
describes starting the console host before deciding whether a visible terminal
is required, including sessions needing no window. CREATE_NO_WINDOW does not
promise that no console support process exists. Job
[ActiveProcesses accounting](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information)
counts associated processes without distinguishing application fixtures from OS
support processes.

A console host explains the observed additional process consistently with that
contract, but its image was **not queried in the failed run**. This is an
inference, not a directly observed identity. No global process enumeration or
new production PID-diagnostic API was added.

Only the two **post-resume** equalities become lower bounds, three and one. The
known fixture processes are independently held and observed alive. Tests still
require:

- Exact observation handles for root, child, grandchild or writer to signal after
  termination; fixture safety-cap exit cannot count as cleanup
- Repeated termination/capture cancellation to work
- Capture cancellation to finish within one second, yield no further bytes,
  avoid fabricated EOF and leave the exact writer alive
- The entire owned Job, including any support processes, to reach **exactly zero**
  after cleanup
- The suspended root to account for exactly one process and execute no user code

The failed tests stopped before their remaining cleanup assertions. Destructor
unwinding from a failure is not evidence that those later assertions passed;
this revision must execute the complete paths in native Windows CI.

An independent review supports this narrowly scoped test correction. We retain
CREATE_NO_WINDOW rather than substituting DETACHED_PROCESS merely to recover
an exact count: detached-console behavior is a different compatibility decision.
Possible console-host overhead remains part of future complete-process-tree
resource measurement; no memory advantage is claimed here.

## Final local checks

- **493 Rust passes** (487 ordinary plus six explicit process tests), zero failures
- Five actual-agent Python chains and two public-export regression tests passed
- Rustfmt, strict Linux and MSVC whole-workspace/all-target/all-feature Clippy passed
- Linux release build and all six tests against the release agent passed
- All 343 upstream package records are unchanged; only workspace package versions
  changed in Cargo.lock

Logs: [aggregate](../PUBLICATION.md#verification-evidence),
[MSVC check](../PUBLICATION.md#verification-evidence),
[release](../PUBLICATION.md#verification-evidence),
[release-agent integration](../PUBLICATION.md#verification-evidence).

Linux release SHA-256 (unsigned, not Windows binaries or reproducibility proof):

- `cedar`: `3d2817c9d80e0b1d47f7396f28d2a17459d42aba6cbdc1f615ca32fcf7d3e5ba`
- `cedar-agent`: `308160e812de47eb1bb78f3db0fae0ad90348227001201dbfd958238b2373789`

## Remaining gate

Publish this audited revision, then verify Ubuntu and Windows on its exact SHA.
Windows must complete both the original thirteen primitive lifecycle tests and
all seven isolated-agent/bundle tests, including no-console and repeated-task
handle checks. No Windows GUI acceptance is implied.

The historical Linux timeout remains unconfirmed; it has not recurred in the
subsequent passing runs and the original four-second bounds remain. Authenticated
SSH and native trust-on testing still await their existing approval. No such
operation or new credential is part of this revision.
