# Verification report · Windows recovery descriptor feasibility / 0.43.0

The latest fully accepted checkpoint is [0.42.1](TEST_REPORT_PHASE421_SAVE_MENU.md). This increment is nonshipping inspection and test coverage only. Recovery storage behavior and its existing Windows ACL limitations remain unchanged.

See [the bounded probe contract](WINDOWS_RECOVERY_PRIVACY_PROBE.md). Focused local checks passed: 43 recovery tests across three suites, including six portable memory-only probe cases; strict recovery-crate host and Windows MSVC-target Clippy; formatting; and 477 Python cases (468 passed, nine existing platform/tool skips), including all 26 fixed-driver tests. The shipping recovery implementation is byte-for-byte unchanged, and non-Cedar locked package data is unchanged. Only the existing pinned Windows API crate is added as a Windows test dependency.

The local host cannot execute this Windows probe. No native descriptor observation, full new-version workspace run or release build is claimed locally. Exact-source Windows inspection, full retained CI gates and all three package builds remain pending. Observed acceptance or rejection of generated inherited descriptors is evidence for a later policy decision, not a claim that production recovery now enforces that policy.
