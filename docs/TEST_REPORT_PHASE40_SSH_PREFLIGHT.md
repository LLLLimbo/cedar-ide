# Verification report · Pure SSH connection preflight / 0.40.0

The preceding 0.39.1 checkpoint passed exact-source Ubuntu and Windows CI and both package audits: public commit `58fe07b624c987e9ac860f749f8d5b25a0f72c93`, [run 38054436623](https://github.com/LLLLimbo/cedar-ide/actions/runs/38054436623). Linux isolated Maven present/missing cases passed in 21,834 ms, with both joined graceful exit-code-zero Stops. The earlier failures remain recorded in the [previous report](TEST_REPORT_PHASE391_LINUX_MAVEN.md).

## Contract

Before an SSH connection form can replace the current worker, it uses the same pure argument validator as the transport. Invalid host syntax, relative remote roots, invalid executable fields or ports fail before connection state is retired. Existing draft, selection, Undo, recovery, profile and trust safeguards remain in force. Building arguments performs no SSH execution, configuration read, host lookup or filesystem probe. Client validation still runs when connecting; no parallel validation policy is introduced.

The form explains that its explicit port overrides an SSH alias's port. Remote paths are literal: shell variables and tilde are not expanded. An absolute agent path is recommended; a bare executable name depends on the remote command environment. Authentication, host-key setup, deployment and actual SSH connectivity remain unverified.

## Required verification

Pure and frontend tests cover malformed and valid literal inputs, manual/recovery entrypoints and preserved state. A normal-agent process case must prove the original owned stdio connection remains usable after rejected SSH forms, with no replacement connection, source changes or execution trust, followed by verified local close. Ubuntu and Windows must execute this case. Existing native Java/Maven, ownership, transport and both package gates remain required.

Local strict host and Windows MSVC Clippy passed, along with 1,418 Rust tests across 45 suites (50 opt-ins ignored) and 385 Python tests (376 passed, nine existing platform/tool skips). Default release app and agent builds passed. The new normal-agent test ran separately: 14 invalid forms, one retained connection, one Hello, one List, 15 Reads, 15 unchanged source hashes and one verified owned-agent close. Four frontend/recovery tests exercise 28 malformed forms and 18 valid literal-path combinations, including selection, two-level Undo/Redo and retained profile/recovery state. Independent lifecycle/recovery review found no remaining issue. These are cloud Linux results; exact-source Ubuntu/Windows CI and both package audits remain pending. No Linux GUI transport change, new capability, download, user-device action or real SSH acceptance is included.
