# 0.75.0 Files IME focus-handoff fix candidate

## Scope and preserved behavior

This stage fixes only Files query/Directory composition ownership across real
egui focus transitions. The original composing field is retained as owner;
actual focus loss invalidates that lifecycle even if focus later returns. The
original field's current TextEdit response and the final pass focus must agree
before its terminal can start automatic settlement. A commit already completed
in an earlier batch retains its original 250 ms deadline after later focus moves.

A bounded handoff-cancellation barrier also prevents a separated Disabled then
late Commit from relabeling old preedit as a fresh direct edit. Native final
bytes are preserved. Clean native byte-changing edits, existing owned same-value
Text/empty-Paste intent, a fresh owner lifecycle, or an actually executed clean
Find can resume search. A batch containing IME cannot become a clean keyboard
edit merely because its Backspace changes bytes. Selection-only changes, focus
movement and idle do not resume the barrier. Provider switch/dismiss clears it.

No TextBuffer adapter or insertion counter was introduced. The two native
single-line String editors and their scalar limits remain. The general input
classifier, Target/router, IME backend, protocol and dependencies are not
rewritten. Literal query/scope validation, one physical flight, latest candidate,
retired/stale reply handling, explicit preview/Open and dirty draft/Undo/history
contracts remain. There is no automatic Read, retry, polling, save or activation.

## Preserved red evidence and focused development validation

The independent investigation and fix sources were still version 0.74.0. The
unchanged twelve cases cover both query→Directory and Directory→query: accepted
commit before later transfer; cancellation before/between pointer events; commit
before/between pointer events; and transfer before a later commit. A thirteenth
case requests an extra discarded pass. On the rebuilt 074 baseline the matrix
was **6 pass / 7 fail**: raw Commit could arm a search for unchanged preedit.
The corrected candidate passes all thirteen with their original assertions.

After the initial owner fix, transfer→Disabled→idle→late Commit separately
reproduced an automatic-search bypass in both directions. The bounded barrier
closed it. First-pass raw events, actual has/gained/lost focus, final text,
composition owner validity, timer/epoch, request IDs/operations and result state
are recorded from complete production-font updates. No response is cached, no
Context is reentered under its data lock, no private IME state is injected, and
extra waiting frames do not conceal first-pass failure.

Final focused results for that still-074 development source:

- **48 Files live tests passed twice**, with **572 identical structured traces**
  per execution. This includes all original 24 Files tests, the original thirteen
  investigation tests, and additional both-direction subcases.
- **212 retained tests passed, one existing process test ignored** across Browse,
  Recent, SearchKeyboard, ProjectTextPreview and BrowsingShell. These overlap the
  Files suite by 24 tests; the union is 236 distinct passing tests.
- Formatting and strict app all-targets/all-features Clippy passed.
- Independent review rehashed all 1,108 tracked source files, the actual test
  executable and logs, parsed both trace sets, and found no unresolved blocker
  within the authorized focus-handoff scope.
- All 43,180 protected old regular files matched. Cargo global usage metadata
  changed only its content hash; size, mode and owner stayed unchanged.

Positive checks include same-value and short Unicode commits; mixed Text/Paste
before and after Commit; a legitimate direct Commit without Enabled/Preedit;
fresh destination lifecycles; accepted commit then transfer retaining 250 ms;
explicit Find; fresh Text/Paste, empty Paste, Backspace and genuine byte-changing
Undo after cancellation. Negative checks include return-to-origin old terminals,
Disabled then late Commit, and late Commit plus native Backspace that genuinely
changes the destination while automatic settlement stays paused. Old replies
release the physical slot without admitting cancelled preedit, and a duplicated
old reply cannot replace current paths or error state.

Two newly written fixture oracle mistakes remain preserved as failed runs. A
4,096-scalar Unicode test incorrectly expected a search despite unchanged byte
limits; its final oracle checks native truncation plus the exact existing
validation error and zero requests. Another new fixture treated selection-only
Undo as a byte-changing edit; the final fixture explicitly preserves that first
negative control, then verifies a second genuinely byte-changing Undo. Neither
mistake was addressed by weakening the original tests or changing production or
protocol limits to fit the fixture.

## Final source and acceptance boundary

The final 0.75.0 source is rebuilt independently from the exact 074 baseline and
the sealed, reviewed fix/test patch before version propagation. All ten workspace
package versions and exact existing process/driver version assertions move to
0.75.0; registry package entries and dependency checksums remain unchanged.
Current documentation is prepared before the final source seal. Packaged guides
keep the usable contract inline and do not depend on source-only phase reports.

This report is sealed **before** final-version full Rust/Python aggregates,
strict workspace Clippy, all-features release, the last default shipping release,
exact default/all-feature executable pins and five original canonical drivers.
Their outcomes require separate source-bound external receipts, not a backfilled
edit of this report. Local commit/export, exact public CI and three-package
acceptance are distinct gates; no future outcome is preclaimed here.

The cloud desktop has no ready-to-use CJK IME. Synthetic raw IME events through
real egui controls are valuable deterministic evidence but **real OS IME is
unverified**. This fix establishes focus-handoff ownership, not a universal
native Commit acknowledgement. A same-field pointer/caret movement can leave
focus stable while egui rejects a commit; that separate seam is outside this
stage's verified scope. No input method is installed for this work. CJK glyph
fallback, if separately opted into an already installed font face, does not
change that IME limitation. The recording worker proves emitted operations,
not real backend traversal or external data transmission.
