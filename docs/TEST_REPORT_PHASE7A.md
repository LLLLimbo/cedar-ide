# Verification report · checkpoint 7A · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar **0.6.1**, protocol **4**, adds the independent `cedar-winprocess` ownership
primitive. **Windows IDE tasks, Git, legacy Run and language services remain
disabled.** There is no TaskManager, workspace capability or frontend activation
in this checkpoint. Windows runtime results are pending at sealing time; this is
a cross-checked candidate for the dedicated Windows CI gate.

The preceding phase-6 public commit
[`f5a0c84f731f66c2f003513e52857fcbf77199a0`](https://github.com/LLLLimbo/cedar-ide/commit/f5a0c84f731f66c2f003513e52857fcbf77199a0)
passed [Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37649215721).
It verified capability discovery, legacy file-only handling, trust separation,
release builds and separate-agent integration. It does not verify the new Win32
primitive. [TEST_REPORT_PHASE6.md](TEST_REPORT_PHASE6.md) remains historical.

## Final local checks

| Check | Result | Scope |
|---|---|---|
| Rustfmt and strict workspace Clippy | PASS | Final Linux source/build |
| Ordinary Rust workspace tests | 467 passed, zero failed, nine opt-in ignored | Linux; includes 13 encoder and one fixture-format test |
| Explicit client/agent integration | Six passed | Selected separately from the nine ignored cases |
| Total Rust passes | **473** | No double counting of focused repeats |
| Python real-agent chains | **Five passed** | Existing filesystem/capability/LSP/task/profile paths |
| Public-export regressions | **Two passed** | Existing original/recovered-source checks |
| Windows MSVC whole-workspace strict Clippy | PASS | All targets/features cross-compile, not runtime |
| Linux release workspace build | PASS | Product behavior still uses existing backends |
| New Windows-only unit tests | 26 compiled | 14 process owner, 10 pipe, two security tests; not executed locally |
| New Windows lifecycle tests | 13 compiled, opt-in on Windows | Dedicated CI step required |
| New native desktop / JDT / performance run | Not performed | Previous evidence is not inherited by a new binary |
| SSH authentication and native trust-on | Not performed | Existing permission gates remain unchanged |

Cargo.lock retains 343 upstream dependencies and now has ten workspace crates.
The Windows-only dependency edge reuses already-locked windows-sys 0.61.2 and its
existing license text; no upstream package version was changed.

Logs: [aggregate](../PUBLICATION.md#verification-evidence),
[Windows cross-target Clippy](../PUBLICATION.md#verification-evidence),
[Linux release build](../PUBLICATION.md#verification-evidence).

Linux release SHA-256 (not Windows binaries or reproducibility attestations):

- `cedar`: `3e88240c107630beaa8ee643389cd3eb3de5f3fe7783b7a3f41a66daa6d49d89`
- `cedar-agent`: `8bd55a0a1266478e0099baa094b5a2c6f2ef9a458fb71e2c141bd5443efc3727`

## Primitive contracts under test

- Windows 10 / Server 2016+ atomic JOB_LIST assignment, suspended creation,
  non-inheritable kill-on-close job and exact stdio HANDLE_LIST
- Aligned attribute-list storage, boxed values surviving list deletion, immediate
  process/thread ownership and independent root cleanup on failed membership
- Explicit single resume; signaled exit detection preserving all u32 codes,
  including 259; no taskkill, PID-based termination or unconfined fallback
- Private current-logon, first-instance, local-only pipes; client PID verified
  before inheritance; no default broad pipe DACL or stored credential
- Pinned OVERLAPPED/buffers, bounded fair capture, truthful EOF, and cancellation
  joined to actual completion before reuse/free; no detached reader threads
- Owned-handle cleanup through partial construction, ordinary Drop and unwinding
- Query/synchronize-only non-inheritable observation duplicates
- Pure two-pass bounded UTF-16 argv encoding, distinct argv[0] rules and 19,608
  short-string oracle cases, plus exact length/escaping boundaries

The host restriction is deliberate: unrelated broad-inheritance spawns can still
take temporary inheritable handles. Supported future activation belongs in an
isolated, controlled-spawning agent, not the GUI process. No TaskManager wiring,
PATH resolution, native batch-file policy, global spawn gate or credential setup
is smuggled into this checkpoint. See [WINDOWS_PROCESSES.md](WINDOWS_PROCESSES.md)
and the [crate contract](../crates/winprocess/README.md).

## Review and test validity

Independent source review checked memory/handle ownership, API lifetimes,
cleanup ordering, cancellation races and security-descriptor bounds. A fixture
bug was identified: readiness before the final pipe writes could let a broken-
pipe error look like successful job cleanup. Idle, leaf and root readiness now
follow all their final pipe writes/flushes; branch readiness is followed only by
pure idle. The reviewer rechecked these paths and found no remaining blocking
source finding. This is not Windows runtime proof.

Fixtures are short-lived, deterministic Rust executables. Owner-crash testing
uses an outer ordinary fixture process rather than a driver-owned job that could
mask the inner job's cleanup. A controlled live descendant is opened only for
query/wait observation; reported PIDs are never used to kill it. Assertions occur
before outer guard cleanup and reject safety-cap exits.

The Windows CI plan runs ordinary unit tests, release build, then all 13 ignored
lifecycle tests serially using the exact release fixture path. Coverage includes
suspended/live-tree Drop, nested cancellation, descendants retaining pipes,
pending read cancellation with writers alive, dual-stream output, literal argv
and cwd, inherited-handle exclusion, owner death and repeated handle counts.
Per-fixture caps, driver watchdogs and CI timeouts bound regression failures.

## Required next gates

1. Publish the audited candidate and obtain actual same-commit Windows results;
   diagnose and repair failures rather than weakening ownership assertions
2. Keep Windows execution unavailable until the primitive gate passes. Then
   implement a separate isolated-agent TaskManager checkpoint and revalidate
   trust, cancellation, output limits, EOF/error cleanup and capability reporting
3. Keep Windows Git, legacy Run and persistent LSP/DAP disabled until their own
   process-tree and cancellable-I/O adoption is complete
4. Preserve the pending authenticated SSH/native trust-on requirements. Local
   process tests do not establish remote-network behavior or permission

No native Windows/macOS GUI acceptance, performance comparison, power-loss or
hostile-code containment guarantee is added. Kernels can delay cleanup despite
requested cancellation; freeing kernel-referenced memory to meet a timer is not
an acceptable workaround. IntelliJ feature parity remains a long-term goal.
