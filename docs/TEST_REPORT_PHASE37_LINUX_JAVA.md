# Verification report · Linux isolated-agent typed Java / 0.37.0

The [0.36 Linux ownership checkpoint](TEST_REPORT_PHASE36_LINUX_OWNERSHIP.md) is fully verified. This slice adds basic typed Java to the Linux isolated agent; the embedded Linux Local workspace keeps generic LSP, and Maven remains Windows-only.

## Final native checkpoint

Public source `8b8617e349f9bb224a7178770d339743852bad46` passed both OS jobs in [CI 38048264198](https://github.com/LLLLimbo/cedar-ide/actions/runs/38048264198). Actual Ubuntu Java acceptance completed in 27,051 ms, with spontaneous correction and no recovery. Initial Stop was graceful/code 0; restart Stop was forced/grace-expired/signal 9 with joined cleanup. The tested agent SHA matched the packaged default-feature ELF. Both archive inventories, source provenance and Linux ABI were verified. Windows ZIP: 5,238,655 bytes, SHA256 `79f596b792c1e5fcd209af218c1d462bd072cd49a8cc78a3bad46460679852d6`. Linux archive: 943,172 bytes, SHA256 `0e293d5aabd7f90072f5a54dde645ffc459245f53ced1d44008db39eb0fc7b2d`. The preparation-time report below retains its original local/native distinction.

## Contract

The existing seven basic Java capabilities bring Linux isolated agents to 31 of the unchanged 32-name limit. Requests, startup IDs, trust checks and fixed launch options remain unchanged. Production Linux selects an explicit native executable named `java`, `config_linux`, an exact relative launcher and an existing data directory outside the workspace. No PATH search, shell, automatic download or Maven import is introduced. JDK/JDT are separately installed trusted tools, not redistributed dependencies or a network sandbox.

Linux Stop reports an explicit code or signal. It requires observed root termination, reaping, released owned I/O, worker join and no cleanup errors. Shutdown quality is separate: a verified nonzero/protocol-error shutdown can retire its owner, while uncertain cleanup blocks replacement. Windows Stop keeps its original body. Use the matching frontend for Linux typed Java: older parsers safely reject the new shutdown shape. Strict typed parsing rejects unknown or mixed fields; duplicate raw JSON keys retain the existing protocol Value-parser limitation.

## Required acceptance

The native workload uses a normal default-feature agent and generated source only. Main budget is 480 seconds, comprising 360 primary and 120 reserved cleanup. Startup keeps its original 75-second envelope, admitting each 30-second Begin/Poll/Read call separately. Typed session calls retain 75 seconds. The original spontaneous diagnostic verdict remains separate from a maximum of one supported explicit refresh after timeout; recovery requires all 240 seconds for refresh, witness and Close. Insufficient remaining budget is a failed acceptance. Same-agent restart has 180 seconds, within one 720-second runtime watchdog. Preparation is bounded separately.

Required witnesses include asynchronous file responsiveness, semantic and editor/Undo behavior, refresh/imports/implementations, truthful Stop, restart, source preservation and process cleanup. All previous Windows and Linux package gates remain required. Exact-source native CI and regenerated package verification are pending.

## Local acceptance

Final local checks passed: 1,372 Rust tests across 44 suites, with 48 explicit opt-ins ignored by the aggregate run; strict host and MSVC Clippy; formatting; and the targeted Python suites (28 driver, 5 capability, 10 Linux receipt, 30 Linux package, and 35 Windows package tests with one platform skip). The real Linux Java opt-in was executed separately as described below.

The generated Linux workload passed on the cloud Debian host in 21,865 ms with the pinned JDT 1.61.0 archive and existing JDK 21. It verified the exact 31-capability inventory and exercised the Java/read subset: asynchronous read responsiveness, semantic diagnostics, editor Apply/Undo/Redo, import organization, two type implementations and one method implementation, explicit refresh and same-agent restart. Spontaneous correction matched; conditional recovery was not attempted. The initial Stop was graceful with code 0. Restart Stop exhausted its unchanged grace period and reported forced cleanup with signal 9 and joined ownership evidence. Source files stayed unchanged, the client was reaped, and the successful fixture was removed.

An earlier preparation attempt failed closed before server execution because the default test harness selected zero tests. The corrected harness explicitly enables the existing shared-fixture feature while continuing to execute the separate, unchanged normal release agent. The failed selection receipt is retained. The local receipt establishes that prebuilt agent's SHA identity, not independent equivalence to the current checkout. Native Ubuntu CI must build the normal agent from the exact published source and establish that linkage.

## Limits

No Linux Maven profile, local embedded typed Java, SSH deployment/authentication, user-device operation, native Windows GUI claim or resource improvement claim. Process groups retain the 0.36 escaped-descendant/abrupt-owner-death limits. Diagnostics may lag; explicit refresh is a mitigation, not an upstream race fix.
