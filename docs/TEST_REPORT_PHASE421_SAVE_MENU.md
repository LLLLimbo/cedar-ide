# Verification report · Save actions label / 0.42.1

The latest fully accepted checkpoint is [0.42.0](TEST_REPORT_PHASE42_SAVE_ALL.md). This narrow follow-up replaces the Save dropdown’s unsupported Unicode glyph with ASCII `...` and the tooltip `Save actions`. Save All behavior, trust, transport, bounds and acknowledgement/recovery semantics are unchanged.

Local formatting and strict host/MSVC Clippy passed. The all-features Rust workspace passed 1,459 tests with 55 opt-ins across 52 suites, including all 40 Save All unit/frame tests. The two new pointer regressions verify the actual hover tooltip and captured press/release identity across frames and repeated menu dismissal; retained tests cover the 780px header bounds and Save All keyboard/transaction behavior. Python ran 451 cases: 442 passed and nine existing platform/tool-dependent cases skipped.

Fresh default 0.42.1 release binaries, actual process gates, exact-source dual CI, all three archives and the final cloud X11 native label/menu check remain pending. No previous release binary is represented as this version. Visual pointer/tooltip verification does not establish semantic accessibility support.

## Final exact-source acceptance

Public source `606953bd480fa9bba224b8fa14e509b817413af7` passed both OS jobs in [CI 38068152801](https://github.com/LLLLimbo/cedar-ide/actions/runs/38068152801). All three archives passed payload, source and provenance verification. Both OSes executed all 40 Save All unit/frame cases and the five actual process receipts. The exact Ubuntu-built desktop passed a cloud Debian X11 native check: ASCII dots and the actual Save actions tooltip rendered, pointer menu controls worked, three generated files saved exactly, the frontend closed normally with exit 0, and all 527 payloads plus the manifest remained unchanged. This is visual/pointer evidence; semantic accessibility, native in-flight cancellation and independent agent reaping remain unverified.

Linux Java corrected spontaneously in 26.306 seconds; its restart Stop was forced SIGKILL 9 with joined cleanup. Linux Maven passed in 30.415 seconds. Windows Java was spontaneous, including the 41.388-second idle workflow, and Windows Maven passed in 33.754 seconds. No upstream diagnostic root-cause fix is claimed.
