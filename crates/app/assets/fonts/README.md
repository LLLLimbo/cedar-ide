# Embedded Cedar fonts

Cedar embeds these two unmodified, static Regular faces. They are not installed
on the host. There is no runtime font download and no paid/proprietary font.

- Inter: official 4.1 release archive, member `extras/ttf/Inter-Regular.ttf`.
  The actual face version is `Version 4.001;git-9221beed3`, which is retained
  unchanged and recorded separately from the release number.
- JetBrains Mono: official v2.304 tag's `fonts/ttf/JetBrainsMono-Regular.ttf`.
  The pinned Git commit, actual face version, hashes and byte counts are in
  `manifest.json`.

Both files have weight 400 and are licensed under SIL Open Font License 1.1.
Their complete, original upstream licenses and copyright notices are adjacent.
Cedar uses no modified/subset/renamed face, and claims no upstream endorsement.
The repository-level `THIRD_PARTY_NOTICES.md` also reproduces both license texts
so the existing binary-bundle packaging includes them without needing loose
font files. The Cargo license collector regenerates this font section from the manifest.

`system_fonts::base_fonts()` places Inter first in the proportional family and
JetBrains Mono first in the monospace family, retaining egui symbol/emoji
fallbacks. An optional installed CJK face is appended to this same base; OS
fonts are never redistributed. `syntax::editor_line_height()` supplies the
shared 1.2 × code-size logical line height for code and the one-point-smaller
gutter. egui rounds each row to the same physical pixel height at the active
scale; the gutter also uses measured glyph-baseline compensation.

Verification commands (run from the repository root):

```sh
cargo test -p cedar-app --lib system_fonts::tests
cargo test -p cedar-app --lib syntax::tests
# Optional, only with a supported installed CJK font:
cargo test -p cedar-app --lib system_fonts::tests::real_system_cjk_fallback_covers_chinese -- --ignored
```

No application launch is needed for these headless font/layout tests. Native
screenshots at multiple UI scales remain a separate visual-acceptance step.
