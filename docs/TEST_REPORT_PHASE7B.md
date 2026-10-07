# Verification report · checkpoint 7B / 0.7.0 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar **0.7.0**, protocol **4**, adds Windows asynchronous tasks in an isolated
agent and routes Windows Local through its bundled sibling agent. The production
integration is a new acceptance target. **Windows Git, legacy synchronous Run,
persistent language services and debugger integration remain unavailable.**

## Verified starting point

The preceding public revision
[`139320e6bb988eb3de4ceecb992d68cd3d0442dd`](https://github.com/LLLLimbo/cedar-ide/commit/139320e6bb988eb3de4ceecb992d68cd3d0442dd)
passed [exact-commit Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37662728974).
Windows executed all 39 primitive library tests and all 13 opt-in lifecycle tests;
Ubuntu's six separate-agent tests also passed. These results are primitive and
previous-version evidence, not proof of the 7B integration.

The earlier 7A Linux lifecycle timeout did not recur. Atomic PID fixture
publication and precise diagnostics remove an observed fixture race, but the
original timeout's cause is still unconfirmed. The four-second condition deadline
and cleanup assertions remain unchanged; a passing rerun does not establish its
root cause. Historical reports retain their sealed evidence.

## Implemented integration

- Immutable internal BackendMode separates InProcess from IsolatedAgent. The
  ordinary constructors default to InProcess and reject Windows tasks. Agent
  host code selects IsolatedAgent; requests, metadata and trust toggles cannot
  change it. Hello is side-effect-free and execution trust remains independent
- Windows advertises only the async task trio in an isolated host. All direct
  task operations enforce trust and host support before lazy supervisor creation.
  Legacy Run, Git and language backend rejections remain in force
- Windows Local uses only the exact cedar-agent.exe beside the current frontend
  executable. Missing/invalid bundles fail clearly, including when valid decoys
  exist on PATH/cwd or in environment variables. Both product binaries must ship
  together; there is no in-process fallback
- The shared bounded task controller retains one active command, eight completed
  records, 256 KiB per stream, 4 KiB errors and a 300-second task maximum. Unix
  process-group/wait-ownership behavior is mechanically preserved
- The Windows owner stays on the supervisor thread. Suspended creation is
  followed by a cancellation/deadline recheck before resume. All outcomes clean
  the job, drain for at most the normal 250-ms budget, and complete cancelled I/O.
  The owner is destroyed before terminal history is published
- Windows requires absolute UTF-8 native .exe program paths. There is no PATH,
  PATHEXT, relative-path, extension or batch-file fallback. Generic request
  bounds fail synchronously; invalid Windows launch paths are retained as
  SpawnFailed, with no execution or implicit retry
- Optional windows_exit_code retains native u32 values. Signed exit_code remains
  populated only for values representable as i32. Old JSON readers ignore the
  new field; Unix snapshots retain their existing shape. The UI displays native
  decimal/hex values and explains Windows program-path requirements
- Bundled agent and task launches use CREATE_NO_WINDOW for stdio-only console
  processes. The new task fixture checks actual console absence and preserved
  stdout/stderr. GUI executables can still display their own windows

## Final local verification

- **493 Rust passes**: 487 ordinary workspace tests plus six explicit separate-agent
  tests; zero failures. Nine aggregate opt-in cases, six subsequently run
- Five real-agent Python chains and two public-export regression tests passed
- Rustfmt and strict whole-workspace, all-target/all-feature Linux Clippy passed
- Strict whole-workspace MSVC target Clippy passed; this is cross-compilation,
  not Windows execution
- Linux release workspace build and all six tests against the release agent passed
- Production integration and final fixture source were independently reviewed;
  no remaining blocking finding was reported

Logs: [aggregate](../PUBLICATION.md#verification-evidence),
[Windows target](../PUBLICATION.md#verification-evidence),
[release](../PUBLICATION.md#verification-evidence),
[release-agent integration](../PUBLICATION.md#verification-evidence).

All 343 upstream package records in Cargo.lock are unchanged. Only workspace
versions and internal/development dependency edges changed. No third-party
executable was added. The new seven Windows agent tests and original thirteen
primitive lifecycle tests must run on the exact published SHA; their successful
cross-compilation is not runtime acceptance.

Linux release SHA-256 (unsigned; not a reproducibility or Windows claim):

- `cedar`: `5918447061dac1e0629d33e91d6e0c4d5411685d5a864c700cee2042719a4da1`
- `cedar-agent`: `12a5b28021cf93396d6f5f1b4047b39cbb29ac567bf1d799a950071699a2de26`

## Windows integration cases

The serial opt-in agent suite uses exact prebuilt native fixtures. It covers:

- Host/capability/trust boundaries and direct legacy-operation rejection
- Responsive Start/Poll/Cancel and file operations while a task runs; busy,
  repeated cancel, stable terminal results, bounded history and reconnect without
  adopting old IDs or automatically replaying commands
- Literal argv/cwd, null stdin, invalid paths/images, native 259/high-bit exit
  codes, split UTF-8, exact stream limits, overflow and complete final suffixes
- Natural root exit followed by descendant cleanup and a distinct timeout cause
- EOF, malformed/truncated/oversized frames, broken response output, client drop
  and forced agent death while an observed descendant tree is alive
- Windows Local metadata, file-error connection retention, explicit trust, task
  routing, exact bundle success and missing/invalid bundle failures with decoys
- Agent handle counts after warmup and 24 completed tasks, queried through the
  exact owned Child handle; this does not establish absence of thread leaks

Controlled live fixture PIDs are opened only for query/wait observation. No PID
is termination authority. Assertions precede harness cleanup, and the agent is
not wrapped in a test-owned job that could hide failure. Owner-death fixture readiness follows final pipe I/O, so reader closure cannot
masquerade as cleanup. The separate gated overflow root intentionally writes
again, while its observed descendants remain idle. The gated natural-exit fixture
permits observation handles to be opened before root exit.
Safety-cap exits are rejected as cleanup evidence. Non-timeout cleanup cases
use task deadlines longer than the fixture cap, so an ordinary task timeout
cannot fake cleanup. Relative-path decoys exist in both the distinct agent cwd
and workspace; cwd identity is compared after canonicalization. Driver and CI
deadlines bound regression failures. This is synthetic program testing, not a hostile-code sandbox.

The original 13 primitive lifecycle tests are also rerun because this checkpoint
changes the child creation flags. Test-only probe/fixture executables are excluded
from product artifact uploads.

## Remaining gates and limits

1. Publish the audited source and verify Ubuntu and Windows CI on that exact SHA,
   including both the primitive suite and new isolated-agent/bundle suite
2. Preserve old Linux timeout diagnostics and investigate a recurrence without
   weakening bounds or claiming a speculative cause
3. Real authenticated SSH and native trust-on tests still need their existing
   approval. No keys, sshd, user-server connection or native trust activation was
   introduced by these ordinary synthetic process tests
4. Windows/macOS GUI acceptance, a new JDT/Kotlin run, production projects,
   performance comparisons and authenticated network recovery remain unverified

The Windows agent and tasks still execute with the account's full permissions.
Exceptional OS operations can delay cleanup; neither a working directory nor a
Job is an adversarial sandbox. IntelliJ feature parity remains a long-term goal.
