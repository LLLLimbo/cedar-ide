# Verification report · Windows recovery descriptor diagnostics / 0.43.1

The latest fully accepted checkpoint is [0.42.1](TEST_REPORT_PHASE421_SAVE_MENU.md). This increment changes only nonshipping inspection and diagnostic tests. Recovery storage behavior and its existing Windows ACL limitations remain unchanged.

## Preserved 0.43 result

[Exact 0.43 CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/38071120608), source `d54e06ab01d7441f6daad0bb54a2b9ef8e7948a5`, attempted the native Windows probe once. Both fresh roots yielded four of five descriptor observations, all eight with owner mismatch, then failed at empty-file rename. Both roots were cleaned up; metadata and draft payload bytes remained zero. The missing native error code leaves the rename cause unconfirmed. The final identity and source-after checks were not reached; their old false fields do not establish observed changes. This failed verdict is preserved.

## Narrow correction

See [the bounded probe contract](WINDOWS_RECOVERY_PRIVACY_PROBE.md). The corrected probe permits write sharing only on the generated rename-destination root handle and preserves its delete-sharing denial. It independently reports owner class and DACL verdict, fixed rename error categories, and explicit identity observation state. The driver checks source independently after a native failure and distinguishes an unperformed check from a mismatch or inspection error. None of these observations authorize plaintext recovery writes or change ACLs.

Focused local checks passed: 48 recovery tests across three suites, including 11 portable descriptor/diagnostic cases; all 40 driver tests; all 491 Python cases (482 passed, nine existing platform/tool skips); strict recovery-crate host and Windows MSVC-target Clippy; formatting and diff checks. Independent handle/policy/receipt review found no blocking issue. Shipping recovery bytes and non-Cedar locked package data are unchanged. The local host does not execute the Windows probe. Full new-version workspace tests, release builds, retained CI gates and all three packages remain CI requirements. One corrected source-bound Windows observation is authorized; there is no automatic retry. Its evidence must receive a separate compatibility and policy decision before any shipping enforcement.
