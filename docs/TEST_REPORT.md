# Verification report · Enter CRLF selection policy / 0.48.2 (pending)

Latest accepted checkpoint: [0.48.1 leading-whitespace Enter](TEST_REPORT_PHASE48_ENTER.md#final-verified-acceptance--0481), exact public source `4b1e2e2963ab1a4e11f6bc67028dd85c3de35c2f` and dual-platform CI 38093773990. Prior Windows EOF-marker and native CRLF failures remain recorded there; the historical EOF failure cause is unknown. Windows recovery admission remains paused.

This proposed Enter-only policy contracts a nonempty selection's upper endpoint from inside CRLF to before CR only when the lower endpoint is valid and strictly before that CR. It preserves the complete boundary CRLF and never adds an unselected byte to deletion. Lower-interior, both-interior, CR-only and LF-only selections still refuse. It cannot infer whether the selection came from Shift+End, mouse input or deliberate raw-CR selection; this is an explicit command policy. Global selection/navigation behavior is unchanged.

The original primary/secondary positions and both affinity flags remain the Undo checkpoint. Reversed selections produce the same text; only the indentation surviving before the unchanged lower endpoint is copied. Existing mixed-newline choice, 1 MiB input/result and 4,096-byte indentation bounds, input ownership and no-op history semantics remain.

Acceptance is pending independent review, pure and actual Shift+End/End→Shift+Home frame tests, normal-agent zero-operation proof on both OSes, all retained CI gates and three packages, then fresh exact-package cloud Trust-off forward/reversed selection Enter/Undo/Redo with unchanged disk and package hashes. No new acceptance is claimed yet.

## Final-source local verification

Independent transaction/input review is clear. All 26 focused Enter tests passed on the first run, including actual Shift+End and reversed End→Shift+Home, every endpoint-affinity combination, cross-line/mixed-newline/EOF cases, effective-size boundaries and no-op redo preservation. Formatting and strict host/MSVC Clippy passed. The complete local aggregate passed 1,586 tests, zero failures and 58 opt-ins across 48 suites. Cargo.lock changes only the ten Cedar workspace versions.

The fresh local default-feature workspace release built successfully. Its normal-agent acceptance executed once with 42 cases and 32 effective transactions, one Hello/List and two setup Reads; measured editor operations, Writes and other commands were zero. Both generated disk hashes, exact orientation/affinity Undo, upper-contraction and retained-refusal witnesses passed; one agent was reaped and fixture/watchdog cleanup completed. These local cloud Debian binaries are not the future CI package. Package/export Python checks passed. Full native Windows CI, three new packages and the exact-archive cloud GUI gate remain pending.
