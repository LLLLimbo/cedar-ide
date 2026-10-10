# Verification report · Save actions label / 0.42.1

The latest fully accepted checkpoint is [0.42.0](TEST_REPORT_PHASE42_SAVE_ALL.md). This narrow follow-up replaces the Save dropdown’s unsupported Unicode glyph with ASCII `...` and the tooltip `Save actions`. Save All behavior, trust, transport, bounds and acknowledgement/recovery semantics are unchanged.

Local formatting and strict host/MSVC Clippy passed. The all-features Rust workspace passed 1,459 tests with 55 opt-ins across 52 suites, including all 40 Save All unit/frame tests. The two new pointer regressions verify the actual hover tooltip and captured press/release identity across frames and repeated menu dismissal; retained tests cover the 780px header bounds and Save All keyboard/transaction behavior. Python ran 451 cases: 442 passed and nine existing platform/tool-dependent cases skipped.

Fresh default 0.42.1 release binaries, actual process gates, exact-source dual CI, all three archives and the final cloud X11 native label/menu check remain pending. No previous release binary is represented as this version. Visual pointer/tooltip verification does not establish semantic accessibility support.
