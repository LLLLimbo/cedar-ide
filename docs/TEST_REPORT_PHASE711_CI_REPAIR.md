# 0.71.1 narrow CI repair — verification candidate

> Subsequent CI outcome: candidate 1 Linux passed, but Windows failed the license-collector test's short-name/canonical-name cwd oracle after public export passed. See [candidate 2](TEST_REPORT_PHASE711_CANDIDATE2.md). This historical report is not a release acceptance.

## Failed baseline and scope

The immutable 0.71.0 public commit
[`a78af4a72c324e4e8e8540503b1073f71ce7aeff`](https://github.com/LLLLimbo/cedar-ide/commit/a78af4a72c324e4e8e8540503b1073f71ce7aeff)
has tree `536d885d16d49daa74ae429507b09c62f588ed65`.
Its [CI run 38507906311](https://github.com/LLLLimbo/cedar-ide/actions/runs/38507906311)
failed: Windows ran 13 public-export tests with three failures; Linux's app
suite had 1,505 passed, one failed and 54 ignored. Subsequent conditional steps
were skipped, the dependent JDT job was skipped, and there were zero artifacts.
The original source, seal and failures are retained. This patch uses an
independent checkout of that verified public tree, not an unmodified rerun.

## Windows: immutable bytes versus checkout bytes

The exporter already reads immutable tagged Git blobs and checks exact approved
TTF and license SHA-256 values. Three new tests incorrectly used working-tree
license bytes as their expected result. Windows checkout converted the text to
CRLF, while the valid tagged blob and export remained LF. The notices assertion
had the same latent mismatch behind the first license assertion.

Only the test oracle changes: every exported font bundle file and notices file
must equal its tagged blob, including byte length and SHA-256. TTF hashes and
base64 transport assertions remain exact. No font, license, exporter production
code or `.gitattributes` normalization rule changes.

Real `core.autocrlf=true` clean/smudge coverage deletes the fixture text files and
restores them with `checkout-index`. A control, both licenses, provenance text,
manifest and notices must actually be CRLF while their tagged blobs remain LF.
Both initial and already-public fixtures export exact tagged bytes, including
when their working files are subsequently corrupted. Conversely, each license
submitted as a true CRLF blob with an exact `-text` attribute must be rejected.
Unknown binary, tampered TTF and altered-manifest rejection tests remain.

## Linux: stale scroll drag surface over a moved toolbar

The original failure's `label()` helper returned no exact text match without
reporting the label or UI state. It did not identify the CI host's font result
or timing, so that particular timing cannot be reconstructed from the log.

A deterministic headless production-frame reproduction delivers the normal
no-supported-CJK-font error through `SystemFonts::tick` on frame two. The error
banner moves the overflow from y52–82 to y105–135. On frame three, the original
native pointer batch at the visible button is retained completely, but the menu
does not open and `Copy draft` is absent. The interaction snapshot identifies
the editor ScrollArea's older background drag surface above the button. egui
registers that drag surface from its preceding viewport and suppresses a click
under a higher drag surface. Delivery on the other five tested boundaries and
the actual installed-font case passed. This demonstrates the mechanism; it does
not prove the CI run had exactly the same font-arrival timing.

The narrow production repair saves the current parent clip, intersects it with
the current editor lane while the existing ScrollArea registers/draws its
widgets, then restores the parent clip. It adds no UI scope or widget identity,
clears no scroll state and keeps the original background-drag setting. It does
not disable gutter/blank-area drag panning, TextEdit selection, wheel scrolling
or scrollbar input. Find retains its existing background-drag policy.

The original seven reproduction cases pass after this change with the original
three viewport/sidebar pairs, without waiting for a stable layout or widening a
viewport. The clip also removes up to the former three-point paint margin at
editor edges; no claim of pixel-identical scroll offsets is made.

Expanded first-frame checks exposed a separate existing error-panel issue:
egui initially clipped a wrapped CJK-font error to its one-row default before
learning its actual height. This patch premeasures the existing wrapped error
label and small Dismiss button using the current style and full panel width,
including a wrapped Dismiss row, before the panel creates its first clip. It
retains wrapping, dismissal, original widget IDs and the existing panel order.
The original three viewport/sidebar pairs at 13/18-point UI fonts now keep the
complete error and Dismiss visible on the arrival frame without a convergence
frame. Find-close shortcut tests explicitly click the editor before invoking
Command/Ctrl+F; this does not claim immediate automatic focus restoration.

## Test observer lifetime and Project repaint boundary

A new test-only ScrollArea observer initially stored an `egui::Response` in
Context temporary data. Response itself owns a Context reference, creating a
strong reference cycle. A deterministic lifetime witness remains owned after
both app and Context are dropped with that implementation (count two instead
of one). The repair captures only Id, rectangle and boolean values during the
same production frame. The identical drop witness then reaches one. Existing
pointer, drag, selection, scroll offset and draft assertions are retained.
This fix is entirely `cfg(test)` and changes no shipping UI behavior.

The first local full Rust attempt was terminated by SIGKILL without an assertion
result. Its cause cannot be established retrospectively. Later bounded default
and two-thread diagnoses both reached the 1.5 GiB app-RSS guard and were stopped;
those runs remain incomplete. The reference cycle is independently demonstrated
by the deterministic lifetime test, rather than inferred solely from RSS. A
subsequent default-parallel run completed without a resource guard but exposed
one Project-row visibility assertion; that failure is also retained.

The Project fixture's asynchronous font result can add an error panel on its
last observation frame. The existing ScrollArea lays out the row at the old
offset, commits its already requested reveal at the end of that frame, and
requests the next repaint. A controlled arrival matrix demonstrates the
boundary: arrivals on frames one through eight satisfy the original ninth-frame
assertion; arrival on frame nine needs the repaint already requested by egui.
This mechanism does not establish the uninstrumented full run's exact timing.
The test observer admits at most that one repaint when actual viewport,
committed offset and complete predicted visibility prove it is pending. A
persistently clipped row still fails; the viewport and final full-rectangle
visibility requirement are unchanged. No production Project reveal code changes.

Another full run exposed a File Structure fixture initialization mismatch.
Unlike production `CedarApp::new`, that fixture started with egui's default
Latin fonts. Its real CJK loader could later install the embedded base plus a
fallback, changing the modal's Latin metrics between its supposedly stable
frames. A controlled default-to-base transition changes a 14-point title from
84.8×16.0 to 87.2×17.0 and fails the original exact geometry assertion. This
reproduces a mechanism, not the uninstrumented run's precise font timing.

Only that fixture now installs `base_fonts()` before its first frame, matching
production startup. Its natural-font case still starts the real asynchronous
loader. Additional receiver/install timing cases deliver a valid bundled
Latin fallback before and after first presentation, checking exact geometry
through the pass where the font definitions are applied. That synthetic face
proves Latin-metric stability, not CJK glyph coverage. Original modal input,
prelayout authority and first-frame geometry assertions remain unchanged.
The Workspace Access, Test Report and in-file Find actual-frame fixture helpers
also initialize the same base faces, without changing their existing viewport,
frame count, selection or geometry assertions. Their natural asynchronous font
loaders remain active; pure logic helpers and other unrelated fixtures are unchanged.

The complete source-bound local gates and the four freshly pinned retained
process proofs pass. Exact-source CI remains the next acceptance gate.

## Version propagation preflight

The first pinned Browse proof passed, but Scoped Search's source/agent admission
still required literal `0.71.0`. Both actual executables were already `0.71.1`;
this was an omitted release-contract update, not an agent mismatch. That failed
proof and its artifact freeze are retained and superseded for final acceptance.

Before sealing a version change, inspect the workspace/package lock version,
all four strict Rust process gates (scoped search, search navigation, duplicate
lines and report triage), all four Python `SOURCE_VERSION` contracts and their
validator fixtures. They now consistently require `0.71.1`; exact source/agent
version equality, platform equality, schemas and operation ledgers remain
unchanged. Historical reports and the synthetic `cedar-example` license fixture
are not release constraints and keep their historical versions. A fresh full
gate and freshly pinned process proofs must follow any such source change.

## Verification status and limits

- Focused export tests: 15 passed, zero skipped, on Linux with real Git CRLF conversion.
- Pre-fix controlled UI reproduction: six passed, one failed; exact failure retained.
- Post-fix original UI matrix: seven passed, zero failed; no settlement workaround.
- Expanded UI interactions: original seven compact-history cases and the extended
  banner, selection, wheel, scrollbar, gutter-pan, Find, Go To, Back/Forward and
  Undo/Redo checks pass. Project arrival boundaries and File Structure stable
  geometry keep their strict original visibility/rectangle assertions.
- Full Rust workspace/all-targets/all-features gate with libtest default parallelism:
  2,235 passed, zero failed, 71 ignored across 50 suites. App: 1,519/0/54.
- Strict Clippy (`-D warnings`) and formatting: passed on the same source.
- Python: 820 tests run, comprising 810 passed and ten skipped, across 30 suites;
  zero failed suites. Export's 15 tests have no skips.
- Real installed CJK opt-in: one passed using the final full-gate executable,
  with unchanged source/artifact hashes before and after execution. Noto Sans
  Mono CJK SC face 7 was read locally; no system font was installed.
- Four freshly pinned default-feature current-artifact proofs passed: Browse
  74 fields/17 operations, Scoped Search 57/10, Search Navigation 57/12 and File
  Structure 45/11. Ordinary agent/version/platform identity and exact ledgers
  were validated. File Structure used the explicit mock LSP, not JDT. These
  selected-artifact runs do not claim the canonical compile driver or GUI ran.
- Exact-source dual-platform CI and three packages: pending for 0.71.1.
- No new native GUI launch or screenshot covers this patch. The previous 0.71
  r3 development screenshot remains a limited earlier structural baseline.

This patch adds no UI feature or search redesign. Automatic menu-focus return,
full IDEA visual/semantic parity, and the previously documented native coverage
gaps remain outside its scope.
