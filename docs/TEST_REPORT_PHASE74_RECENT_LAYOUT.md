# 0.74.0 Recent Files layout candidate

## Scope and preserved contracts

This stage changes only the Recent Files presentation and its focused tests.
Rows have a primary filename, muted parent and separate Open/Closed badge;
the chooser has compact title/query/status controls, an original selected-path
band and a fixed footer. The primary action reserves the longest Open closed
file label. Empty history and a nonempty history with no local matches are
different states. No fake provider, tabs, preview or edited-files-only feature
is introduced. Files, Project Text, the main chrome, backend and protocol are
unchanged from the previous source checkpoint.

Recent still records at most 64 exact paths in this connection, with the existing
256 KiB total and 4,096-byte individual limits. Reopening clears the local filter
and its native Undo and defaults to the most recent other file. Single, double
and triple row clicks only select; Enter/Open performs ordinary Open. A live
dirty file reuses its buffer without Read; a closed file does not restore an old
caret, selection or draft. Clear removes only the recent list, not files or tabs.
Cancel, owner/generation/epoch checks, IME/quarantine, dirty Undo and reconnect
semantics retain their existing contracts. Display strings never reconstruct
paths or authorize actions.

The only added state is one bounded presentation record containing pure view
and row Id tokens. An owned keyboard/query reveal can survive a discarded pass
until one visible consumption; later wheel/press, selection mismatch or dismissal
clears it. One fixed ScrollArea Id is reused, and a fresh view resets that native
state, including kinetic velocity and pending target. It is not a new request,
input or action state machine and stores no Response, Ui or Context reference.

## Focused development results and corrections

The exact development source was still version 0.73.0. It passed **381 focused
tests, zero failures and one existing ignored process test**: Recent 35, Files
99, Scoped Search 19, Text Preview 18, Search Keyboard 43, Browsing Shell 17,
Workspace Access 33, Location History 70 and Navigation 47. All original 18 Recent
tests and assertions remain unchanged; 17 new presentation tests were added.
Formatting, diff checks and strict app all-features/all-targets Clippy passed.
Independent read-only review accepted this limited stage. These are not final
0.74.0 full-workspace, canonical, native or public CI results.

Production base-font geometry uses Inter and JetBrains Mono at Body/Button 13
and 18 points, actual first-visible and settled update frames, normal 1178x814
and minimum 780x540 sizes. The 700x480 cases are resilience only. The tests cover
0/1/3/64 entries, local filtering/no-match, current/open/closed/stale rows,
duplicate basenames, long ASCII and Unicode paths, real pointer/key/IME events,
semantic output, clipped paint bounds, fixed footer slots, reopen/query Undo,
font-install boundaries and one-shot reveal versus later wheel input. At normal
size the list has twelve rows; at minimum size and 18 points it has nine fully
visible rows. Native screenshots do not establish those capacity measurements.
Controlled font installation tests Latin-metrics receiver/install timing, not
real CJK glyph coverage.

The retained failure traces distinguish product defects from observer mistakes.
Native egui tooltip Area sizing left an invisible hit rectangle over a row and
swallowed a later press. Recent now measures and wraps a full galley and paints
a screen-clipped popup frame without an Area, Ui, focus or hit-test widget. Native
click-after-move suppression and smooth-scroll stillness gates are retained;
cross-widget tooltip grace switching is not implemented. Common long-path
hover bounds and the first real press after hover are dynamically checked.

Requested discard also exposed a lost one-shot End reveal; the bounded token
fix preserves only presentation intent until a visible pass. Reopening after
scrolling exposed a hidden default-other row. Unique per-view scroll salts were
rejected because they accumulated persisted state; 32 reopen cycles now verify
bounded storage. Real drag momentum initially survived dismissal and moved the
fresh view: offset 136.4/velocity -1025.7 before close and offset 10/velocity -999.7
on reopen. The final first reopened visible frame is offset 0/velocity 0. An
initial reset used a differently hashed Id salt and failed three tests; that
trace remains, and the corrected exact native salt passes all 35 Recent tests.

Observer errors involving prior-pass Response flags, fade opacity, post-consumed
smooth scroll and native click-to-move timing are recorded separately. Only pure
numeric/Id probes are retained in Context data. No assertion or first-visible
geometry check was removed, and extra settled frames do not substitute for the
first actual visible pass or the first pointer press.

## Limited native comparison and exact provenance

One independently collected cloud Linux normal-window run used generated
non-user files, Trust off and a 1178x814 client. The parent compared actual
original images and accepted the visual direction. Twenty original JPEGs were
retained without cropping, resizing or re-encoding. They show a stable chooser
and footer across empty, three-row and two-row local-filter states; readable
filename/parent distinctions; separate Open/Closed status; and an unchanged
Clear/Close position when the primary label becomes Open closed file.

A normal CJK sample parent and full hover path were readable. The first hover
observation had no tooltip and a later frame showed it; this does not measure
the exact delay or instrumented first paint. A subsequent raw single click at
the previous tooltip-covered tests-row location selected Closed tests without
opening it, with the active source file unchanged. Local filtering without
Enter, Escape, reopen-clears-query/default-other, explicit Enter to the exact
source file and subsequent MRU/default-other changes were observed. No Clear,
closed-file Open, editor mutation or user-computer action occurred.

The single actual launch exited normally with rc0 after **303.617 seconds
Connected** and 356.676 seconds app lifetime, without TERM/KILL. Complete
postflight verified source/package/pair/fixture and historical-evidence hashes,
owned process disappearance and native Cedar/Terminal window closure. Native
frame metadata is tool observation, not instrumented first-visible timing.

Before that unique launch, preflight correctly stopped before extraction,
startup or Connect because the handoff's all-directory 0700 claim was false:
one intermediate frozen public-license directory was 0755. The standalone
erratum records the mkdir-parent mode omission, unchanged 518 license files,
private package root and lack of group/other write. Frozen permissions, archive
and manifests were not changed. An authorized new runtime extraction explicitly
checked all directories 0700, ordinary files 0600 and executables 0700. The later
success was the original sole launch, not a retry of a failed native run. A
capture timestamp parser discrepancy is retained separately in the capture
ledger; the original image was already saved and was not recaptured.

This native pair is the **0.73.0 development candidate**, not final 0.74.0:

- Private base: ed88ee6e8e34e71dd738bb774b4b32dd4f61e5d1.
- Development source inventory: 1,107 files,
  6ba55ee61d926d8a87fd8ffe398dd1f6dabc5efcda3c1365a735bc6f4098ab24.
- Source seal03 SHA256:
  94a48847fa7b20232bb5f6bc623bda3c33ad75ae93b513c70956563d571f8acf.
- Frontend SHA256:
  e2c7e865eaf0b8641cbbce548837137ed0048ba5d52898ef293c44459254a640.
- Normal agent SHA256:
  3320801e40ae98087252bf98f65ba5bb8f4ec3280fc9aff2112308904bb91783.
- Native report SHA256:
  b45306d664e26d17b9fc3c73d56076773b2147ce93e3b4bc963aa287c5aedcb3.
- Native delivery SHA256:
  85343829f7525fdaaccf7246d0fa2ee44faa472b179ed570a21fb98f46b04b81.
- Standalone permission erratum SHA256:
  d2eea02562acb1b4b5257bbaa54d62542946b13c8c9aa0bd3968b47a37a71b75.

Raw screenshots and machine receipts remain outside the public source snapshot.
The old 073 baseline did not show a wrapped deep-CJK row; that behavior is not
invented to amplify the new comparison.

## Final-version validation attribution

Final 0.74.0 is prepared in an independent source copy, preserving the accepted
development source and all its frozen artifacts. Production Recent code and
tests are unchanged from that candidate. Version propagation touches the
workspace version, ten local Cargo.lock entries and existing exact process-test/
validator version guards. Registry versions/checksums and dependencies remain
identical. Current documentation separates this candidate from historical stages.

This source report is sealed before final 0.74.0 execution. Full default-parallel
workspace/all-targets/all-features Rust, all Python validator suites, formatting,
strict workspace Clippy, aggregate all-feature release and last default-feature
release require fresh external receipts. Complete aggregate testing and both
exact feature prewarms use the real new source/target directory, avoiding a
redundant debug dependency graph without changing the test scope. All-feature
release and default release retain separate reusable caches and artifact
identities; a cache name is not source provenance.

The five existing Browse, Scoped Search, Search Navigation, File Structure and
Report Triage drivers retain their canonical paths, schemas, counts, strict
oracles and compile/runtime/cleanup deadlines. Prewarming never relaxes their
own limits. The default and all-feature app test variants and backend test are
selected from exact Cargo JSON records, not whichever same-named binary was
last built. File Structure continues to use its explicit mock LSP, not real JDT.
New normal-pair/frozen/canonical copies require before/after identity verification.
No anticipated full-gate count is written here, and external results do not
retroactively turn the earlier development images into final-version acceptance.

## Remaining limits

Normal-window native sampling is not narrow/large-font acceptance, 64-row native
scroll, arbitrary CJK/IME/grapheme coverage, query Undo/dirty editor proof, native
backend-operation counts, platform accessibility, Windows/macOS GUI or real SSH.
Rows/path bands retain full layout strings while visually eliding them; this does
not prove all original characters are visibly painted. A 4,096-byte path can
exceed the noninteractive tooltip's physical screen height and clip its tail.
No interactive tooltip or tiny type was added to hide this limit. Public CI,
package audits and any final-pair GUI check remain independent later gates.
Cedar does not claim full IntelliJ IDEA parity or pixel equivalence.
