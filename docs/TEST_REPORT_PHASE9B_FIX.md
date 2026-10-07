# Verification report · checkpoint 9B fixture fix / 0.8.3 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

This is a targeted test-fixture correction after the [9B checkpoint](TEST_REPORT_PHASE9B.md).
Production transport, three-second EOF assertion, workspace trust and the disabled
Windows IDE LSP gate are unchanged.

## Actual previous native result

Public 9B commit
[`e77756a1c0fe5bcb8c4f44ea61b9c7f1ded57fb5`](https://github.com/LLLLimbo/cedar-ide/commit/e77756a1c0fe5bcb8c4f44ea61b9c7f1ded57fb5)
passed Ubuntu CI. Its [Windows job](https://github.com/LLLLimbo/cedar-ide/actions/runs/37689853977/job/113026939697)
passed all 21 owned-process lifecycle cases, including the corrected stdin
completion/exit regression. Eleven of twelve native language-transport cases
passed, including repeated handle/thread counts, root-exit tree cleanup, final
buffered response, blocked writes, frame deadlines and stderr flood. The stdout
EOF/live-descendant case timed out. The subsequent seven agent cases were skipped,
not passed. Neither this failure nor the original 9A failure is hidden by a retry.

## Diagnosis and correction

The synthetic root receives inheritable standard handles. Its descendant helper
uses Rust's ordinary Command with stdout set to NUL and stderr inherited.
[Rust 1.99's Windows implementation](https://raw.githubusercontent.com/rust-lang/rust/1.99.0/library/std/src/sys/process/windows.rs)
uses broad handle inheritance and duplicates selected standard streams; selecting
NUL does not remove the original inheritable root stdout from the unrelated
handles eligible for inheritance. Descendants could therefore retain an extra
copy of the root's stdout pipe, preventing the EOF the test intended to create.
This is a fixture-construction defect, not evidence that production ignores EOF.

Before this fixture spawns descendants, it now uses
[SetHandleInformation](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-sethandleinformation)
to clear only the original stdout handle's inheritance bit and verifies the bit
with GetHandleInformation. The explicitly selected NUL stdout and inherited stderr
still apply. This changes only the synthetic process's owned handle, with no
system setting, permissions or production spawn change.

The test requires the verified-flag marker, exclusive lifetime locks proving root,
child and grandchild are all alive before stdout closes, and a terminal reason
specifically identifying stdout EOF. It retains the original three-second bound
and then requires all lifetime handles released without watchdog expiry. The
fixture feature alone adds its direct Windows API dependency.

## Local validation

- Whole-workspace aggregate: 539 passing Rust executions, zero failures; two
  export regressions and five Python agent/tool protocol chains passed.
- Strict whole-workspace/all-target/all-feature Windows MSVC cross-clippy passed.
- Linux release whole-workspace/all-feature build passed.
- Independent source review agreed with the fixture inheritance diagnosis and
  the narrowly scoped correction.

Raw local logs: `verification-phase9b-fix-aggregate.txt`,
`verification-phase9b-fix-windows.txt`, `verification-phase9b-fix-release.txt`.
Raw machine-specific logs are excluded from public source exports.

## Remaining gate

A new exact-commit native run must execute all twelve transport, twenty-one
process-lifecycle and seven isolated-agent cases. The fixture correction is not
called a native pass until that succeeds. Windows IDE LSP stays disabled pending
native transport plus real Java and agent ownership acceptance. No GUI Trust-on
or authenticated SSH test was performed.
