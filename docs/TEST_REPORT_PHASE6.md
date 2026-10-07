# Verification report · phase 6 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar **0.6.0**, wire protocol **4**, adds bounded agent capability discovery,
immutable first-handshake caching and support-aware frontend controls. This is
an independent IDE under development, not IntelliJ IDEA feature parity.

**Final local checks passed:** 459 Rust tests, five Python agent chains, two
public-export regression tests, formatting, strict Linux/Windows-target Clippy,
and the Linux release build. The exact release also passed a native trust-off
capability/read/edit/save check. Public phase-6 publication and same-commit
Ubuntu/Windows CI are pending at this source checkpoint's sealing time.

## Recovered baseline

Phase 5 is public as
[`119d5c30ddba51cbfa9f04d5ae6e281103a3803f`](https://github.com/LLLLimbo/cedar-ide/commit/119d5c30ddba51cbfa9f04d5ae6e281103a3803f),
with [both CI platforms passing](https://github.com/LLLLimbo/cedar-ide/actions/runs/37643452204).
That includes actual Windows ordinary-file replacement and transport-fault
regressions, release builds and separate-agent integration. It is not Windows
GUI acceptance and does not verify the new phase-6 changes.

After the development environment changed, the complete 654-file phase-5 public
tree was recovered by exact Git tree SHA. The new environment then repeated its
419 Rust passes and four Python agent chains. No missing source was recreated
from memory. Raw excluded historical evidence was not recovered; see
[ENVIRONMENT_RECOVERY.md](ENVIRONMENT_RECOVERY.md). Historical reports retain
what was known when they were sealed.

## Final verification matrix

Environment: Linux x86_64, Rust 1.99.0. No new upstream dependencies were added;
Cargo.lock retains 343 registry dependencies and nine workspace crates. A client
test dependency now directly references the already-locked serde_json crate.

| Check | Result | Boundary |
|---|---|---|
| Rustfmt | PASS | Final source |
| Strict workspace Clippy, all targets/features | PASS | Linux compiler/runtime environment |
| Ordinary workspace tests | 453 passed, zero failed, nine opt-in ignored | Final source |
| Explicit separate-agent/config/fault integration | Six passed | Included among the nine ignored cases above |
| Total Rust passes | **459** | Focused reruns are not counted twice |
| App subset | 248 passed, two opt-in ignored | Included in the total |
| Python real-agent chains | **Five passed** | Filesystem, capabilities, mock-LSP bridge, asynchronous tasks and saved profiles |
| Public export tests | **Two passed** | Initial and previously exported source trees |
| Windows MSVC workspace/all-target strict Clippy | PASS | Cross-target compilation, not Windows runtime |
| Linux release workspace build | PASS | Binary hashes below |
| Native Linux trust-off check | PASS | Exact final release, synthetic workspace |
| New real JDT/Kotlin/debugpy acceptance | Not run | Earlier evidence remains historical |
| Actual authenticated SSH | Not run | Separate approval still required |
| Native trust-on Run/Cancel/reconnect and dirty-close | Not run | No inferred approval or pass |
| Windows/macOS native GUI | Not run | Remains unvalidated |
| New performance/IDEA comparison | Not run | No phase-6 memory claim |

The three opt-ins not executed by the final script remain ignored. The frozen
old/new protocol readers are deterministic compatibility tests, not a real
network or all-versions interoperability certification.

Logs: [aggregate](../PUBLICATION.md#verification-evidence),
[Windows-target Clippy](../PUBLICATION.md#verification-evidence),
[release build](../PUBLICATION.md#verification-evidence).

Final Linux release SHA-256:

- `cedar`: `392f081efc142d2baa0fa4b01584a6b1af55db544a6cd352af6c2e9ee760be1f`
- `cedar-agent`: `ba9ded532d36390182d96961577a7d69dabb65ef23840cf2cb08c43df3484992`

These are artifact identifiers, not signatures or a reproducible-build claim.
Later builds must record their own hashes.

## Capability and connection evidence

- Optional nested schema-1 metadata remains compatible with a frozen old
  protocol-4 reader. Product version never bypasses exact protocol validation
- Missing/null metadata selects only the four legacy file operations; explicit
  empty metadata is distinct. Malformed recognized fields, raw duplicate JSON
  keys, unknown schema, bounds and duplicate capabilities are rejected
- Well-formed unknown names/fields are tolerated without adding executable
  client behavior. Platform/version labels are unverified support claims
- Client caches the complete first validated Hello, including root. Subsequent
  Hello requests do not send another frame; known disconnects take precedence
- Missing support fails before transport I/O while preserving the connection;
  task/LSP startup requires the shared minimum lifecycle groups
- Workspace metadata is identical with trust on/off and starts no tools. Actual
  backend compile-time support replaces frontend-OS/SSH guessing
- A real stdio agent reported version 0.6.0, linux/x86_64 and 21 capabilities;
  untrusted direct requests still returned run_disabled, and the disposable
  workspace remained empty

The deliberate legacy policy change requires upgrading metadata-free agents to
restore advanced tools. It does not assert that those old agents lacked the
implementation. See [REMOTE_CAPABILITIES.md](REMOTE_CAPABILITIES.md).

## Frontend and native evidence

Headless regressions cover stale, cancelled and duplicate Hello success/error
handling; typed-root/recovery checks before metadata installation; read-only
and missing-search behavior; queued profile actions; no-Write Save with zero
editor/form/revision/baseline mutation; independent optional LSP capabilities;
required completion resolution; and reachable active Stop/Cancel paths.

A review found a misleading clickable remote-outline row when URI resolution
was unavailable; its affordance now matches the blocked dispatch. Maximum legal
metadata at the minimum 780×540 window is tested: the agent label is bounded and
ellipsized, full details are in the tooltip, and recovery status stays visible.
Independent code review found no remaining blocking issue in the protocol,
workspace, client and app changes.

The final release was operated in a new synthetic local workspace with execution
trust off. The footer and tooltip showed reported version/platform, supported
Save/Search/Tasks/Language and the separate **execution trust: off** state.
Command Run and language Start remained disabled. Opening, editing and saving a
synthetic README succeeded, with exact disk bytes checked afterward. No tool or
language server was started.

Evidence: [native record](../PUBLICATION.md#verification-evidence),
[capability tooltip](../PUBLICATION.md#verification-evidence),
[trust-off save](../PUBLICATION.md#verification-evidence).
These checks do not stand in for authenticated SSH, legacy-peer native-window
acceptance, native trust-on execution, Windows GUI or native dirty-close tests.

## Export/recovery correctness

The public exporter now accepts a restored public checkout containing an older
PUBLICATION.md. It regenerates that file exactly once and avoids accumulating
README footers or evidence notes. Two temporary-repository tests compare the
entire emitted tree against Git's own write-tree hash, including the recovered
source case. No raw logs, screenshots, tool caches or binaries are added to the
public source export.

## Remaining work

1. Publish the audited phase-6 source tree and verify both platforms on its exact
   public commit. A previous green build is not this checkpoint's result
2. With the specific pending approval, complete authenticated OpenSSH and native
   trust-on tests; keep honest unknown-outcome and remote process-cleanup limits
3. Add Windows Job Object/cancellable-pipe support before enabling Windows local
   Git/commands/language processes; actual remote support remains backend-led
4. Continue project-model, safe refactoring and integrated-debugger work without
   bypassing snapshot/resource-operation or listener/process-tree boundaries

Java/Kotlin language services may still require a separate JVM. No new resource
measurement or comparison against IDEA was made. Historical frontend and JVM
figures must not be presented as a simultaneously measured total; see
[PERFORMANCE.md](PERFORMANCE.md) and [FEATURE_MATRIX.md](FEATURE_MATRIX.md).
