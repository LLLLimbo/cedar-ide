# Verification report · checkpoint 8 / 0.8.0 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar 0.8.0 adds explicit disk comparison and clean-document reload. Protocol
remains 4; existing Read operations are reused. It grants no execution trust and
adds no watcher, merge engine, force save, automatic command or LSP startup.

## Behavior and regression scope

The selected document's draft and reviewed disk snapshot appear in two read-only
panes. Refresh is explicit. A dirty document may be compared but cannot reload.
A clean reload reads again and requires the same text and revision; if disk
changed, only the preview updates and a fresh click is needed. The final input
barrier rejects same-frame typing, stale document identity/version, navigation,
save/close conflicts and queued profile actions. Accepted disk text becomes the
saved baseline in a separate step from the native undo transaction. Undo restores
old text as dirty; Redo returns to the new clean baseline.

Tests exercise stale success/error, dismissal and reconnect, malformed paths and
revisions, oversized content, missing files, no-op/revision-only changes, one
outstanding read even after dismissal, font-atlas/CJK cache invalidation,
actual frontend input ordering, profile-form preservation, recovery ownership
and LSP debounce/manual-sync behavior. No disk review claims a save acknowledgement.

Review found and corrected queue flooding through dismissal, stale response
suppression hiding a current transport failure, and reuse of text layouts across
font-atlas replacement. Native testing then found that egui includes selection in
its undo-state equality, so refocusing or moving the cursor could clear Redo after
reload/Undo. The first correction added same-text cursor checkpoints, which review
found could evict text history after repeated movement; final regressions and
native testing cover
both Redo preservation and text-history retention, including every partial Undo
split of sixteen saved states plus the native uncheckpointed typing state.
The bounded native history
remains the only persistent undo store.

## Native Linux validation

The initial release candidate was tested in a disposable project with Trust off.
It passed identical-snapshot no-op, Chinese disk preview, clean reload, Undo,
dirty reload rejection, changed second-read refresh, explicit second confirmation
and missing-file preservation. On that candidate, reopening/closing review and
refocusing the editor lost Redo. These observations are historical evidence of the
bug, not proof of the corrected binary. The dirty candidate window and its exact
recovery record were retained: original draft A, new disk/base B and SHA256(B),
with disk B unchanged.

Candidate images: [comparison](../PUBLICATION.md#verification-evidence),
[dirty draft](../PUBLICATION.md#verification-evidence),
[changed second read](../PUBLICATION.md#verification-evidence).

The final release was launched separately against a fresh disposable project and
private recovery directory. It passed the same no-op/CJK/clean-reload paths,
forty arrow movements before Undo, dirty review with exact backup acknowledgement,
then review Close, pointer refocus, forty more arrow movements, selection change,
an idle interval and Redo back to clean disk text. A subsequent real text edit
correctly invalidated that old Redo branch. The dirty recovery record was checked
against exact original/modified draft bytes and the new disk/base revision; project
bytes remained unchanged.

The final binary also repeated the changed-second-read path, explicit second
confirmation and missing-file preservation. The externally renamed fixture was
restored. Both dirty test sessions were retained. The fresh test recovery directory
initially had the fixture creator's default permissions and was correctly rejected;
after restricting that new fixture directory to owner-only access, Retry recovery
succeeded. No execution trust was enabled.

Final images: [dirty review and backup](../PUBLICATION.md#verification-evidence),
[Redo after navigation](../PUBLICATION.md#verification-evidence),
[second-read race](../PUBLICATION.md#verification-evidence),
[missing file](../PUBLICATION.md#verification-evidence).
The [structured native record](../PUBLICATION.md#verification-evidence) identifies
the tested binaries and exact scope.

## Final automated checks

- **521 Rust passes**: 515 ordinary plus six explicit process tests, zero failures
- App: 277 passes, two existing opt-in tests ignored; included in the total
- Five actual-agent Python chains and two public-export regression tests passed
- Rustfmt and strict Linux/MSVC whole-workspace, all-target/all-feature Clippy passed
- Linux release build and all six tests against its release agent passed
- All 343 locked upstream package records unchanged; ten workspace versions bumped

The aggregate lists nine ignored opt-in cases; six process tests then ran explicitly.
The remaining three require an external Java/debug runtime or a system CJK font;
those opt-in test functions are not claimed passes.
An earlier aggregate passed before the final seventeen-state boundary correction;
its logs are retained as pre-final evidence and are not the final acceptance run.

Final logs: [aggregate](../PUBLICATION.md#verification-evidence),
[MSVC check](../PUBLICATION.md#verification-evidence),
[release](../PUBLICATION.md#verification-evidence),
[release agent](../PUBLICATION.md#verification-evidence).

Linux release SHA-256 (unsigned, not Windows executables or reproducibility proof):

- cedar: `bf25c3ceb853fb3b24ae97754e6437b85b1c1b6f49956d04bf074eb4eb3ec3b3`
- cedar-agent: `ac1c6e8a3d84a340014324839f92b7968f18df960661d3b16f9ad456a57e5398`

## Platform and remaining boundaries

The previous public revision
[`093cd664e1d89c05ebca11da5b44bf3f2a6c2a5f`](https://github.com/LLLLimbo/cedar-ide/commit/093cd664e1d89c05ebca11da5b44bf3f2a6c2a5f)
passed [Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37673873489).
Windows actually ran all thirteen process-lifecycle and seven agent/bundle tests,
including exact owned-descendant termination. This stage requires a new exact-SHA
CI run; prior results cannot establish the new binary's acceptance.

The historical four-second Linux lifecycle timeout has not recurred; its original
cause is still unconfirmed. Native Windows/macOS GUI, authenticated SSH and the
previously pending native Trust-on test remain unvalidated. No test key, SSH
server, user server connection or GUI Trust-on action was used here. There is no
new memory benchmark, IDEA comparison or full IDE parity claim. Windows Git,
legacy synchronous Run and LSP stay disabled; the next core language milestone
requires cancellable stdin, complete I/O ownership and real Windows JDT testing.

Manual review is not automatic detection. Dirty conflict resolution and durable
history of old disk contents are deferred; undo is in memory. The second read
narrows a race, but an external writer can change disk afterward. Normal saves
retain revision checks and cannot offer filesystem-wide atomic compare-and-swap.
