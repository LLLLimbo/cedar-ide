# Verification report · Tab keyboard focus / 0.19.1 · 2026-10-08

This small follow-up fixes the tab-focus limitation observed during 0.19 native
acceptance. An explicit click on a document tab, including the already active
tab, transfers keyboard focus to that editor. Ordinary frames do not request
focus, so intentional Find/Replace field editing is preserved.

The previous [0.19.0 exact Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37827753911)
and portable ZIP byte verification passed. Its literal-replacement, atomic Undo
and native Linux evidence remain in the [phase-19 report](TEST_REPORT_PHASE19.md).
The intermittent upstream JDT diagnostic-publication cause remains unresolved.

## Scope

Tab activation keeps the existing document object, text, cursor/selection,
saved baseline and native Undo/Redo history. It uses the existing editor focus
ID after the existing navigation invalidation. Navigation dialogs and foreign
confirmations prevent background tab activation and focus changes.

There is no backend, protocol, filesystem-write or permission change. The fix
does not introduce automatic focus on idle frames or alter asynchronous opens.

## Verification status

Six actual-frame regressions passed for pointer activation followed by
typing/Undo, independent dirty-buffer histories/cursors/Redo, deliberate same-tab
activation versus Find/Replace focus, late read replies, and modal guards.
The final local aggregate passed 814 Rust tests across 40 suites, with 22 opt-in
cases retained for their explicit process/native CI stages. Strict host/MSVC
Clippy and all required Python checks passed. The corrected normal release
build passed; its agent bytes match the four previously passed actual-agent
smoke suites. Corrected native Linux trust-off testing passed; exact
dual-platform CI and the regenerated Windows bundle remain pending. Native Windows/macOS
GUI and authenticated SSH are separate gaps.

The first native candidate still lost focus because a fast click batched pointer
press and release into one frame. A new regression reproduced that failure;
the corrected implementation requests focus only after the editor handles the
outside pointer press. The final aggregate includes that passing regression.
The failed candidate is retained separately and is not accepted as the fix.

## Corrected native Linux regression

The exact corrected release passed different-tab immediate Undo, Redo and
typing, independent dirty histories and per-document cursor restoration.
Same-tab activation from Replace and different-tab activation from Find both
focused the editor; intentional field typing retained its focus across ordinary
frames. Both synthetic buffers returned clean, their disk hashes were unchanged
and execution trust stayed off. The test did not save, run commands or use SSH.

Corrected app SHA-256:
`a1c36f676e1e48f7faa997666a2dcd7282aa5834f0e54e3fda9d3d94850a91c6`.
Adjacent normal agent SHA-256:
`9f7a98b5596cce2ebfb8ccc34b5877775802193cd98c8cb916a2069ac2df6878`.
