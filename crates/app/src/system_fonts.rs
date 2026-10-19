//! Embedded Latin UI/code faces and optional local-only CJK fallback.
//! No fonts are downloaded at runtime or bundled from the OS.
use ab_glyph::Font;
use eframe::egui;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
const MAX_COLLECTION_FACES: u32 = 32;
const FALLBACK_NAME: &str = "cedar-system-cjk";
const UI_FONT_NAME: &str = "cedar-inter-regular";
const CODE_FONT_NAME: &str = "cedar-jetbrains-mono-regular";
const UI_FONT: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const CODE_FONT: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Build the same embedded base at startup and when a system fallback arrives.
/// Keep egui's symbol/emoji faces behind our primary Latin faces.
pub fn base_fonts() -> egui::FontDefinitions {
    let mut definitions = egui::FontDefinitions::default();
    for (name, bytes, family) in [
        (UI_FONT_NAME, UI_FONT, egui::FontFamily::Proportional),
        (CODE_FONT_NAME, CODE_FONT, egui::FontFamily::Monospace),
    ] {
        definitions
            .font_data
            .insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
        definitions
            .families
            .entry(family)
            .or_default()
            .insert(0, name.into());
    }
    definitions
}

struct LoadedFont {
    bytes: Vec<u8>,
    index: u32,
    family: String,
    path: PathBuf,
}
enum Status {
    Unchecked,
    Loading(mpsc::Receiver<Result<LoadedFont, String>>),
    Finished,
}
pub struct SystemFonts {
    status: Status,
}
impl Default for SystemFonts {
    fn default() -> Self {
        Self {
            status: Status::Unchecked,
        }
    }
}
impl SystemFonts {
    #[cfg(test)]
    pub(super) fn probe_state_for_test(&self) -> &'static str {
        match self.status {
            Status::Unchecked => "unchecked",
            Status::Loading(_) => "loading",
            Status::Finished => "finished",
        }
    }

    /// Exercise the production result receiver at a chosen UI frame boundary.
    /// Keeping the sender alive prevents a host font scan or an early disconnect.
    #[cfg(test)]
    pub(super) fn pending_failure_for_test() -> (Self, impl FnOnce()) {
        let (tx, rx) = mpsc::channel();
        let deliver = move || {
            assert!(
                tx.send(Err("No supported installed CJK font was found within the 32 MiB limit. Chinese text is retained, but some characters may appear as boxes. Install a system CJK font and restart Cedar; no fonts were downloaded".into())).is_ok(),
                "the controlled font probe must still have its receiver"
            );
        };
        (
            Self {
                status: Status::Loading(rx),
            },
            deliver,
        )
    }

    /// A valid bundled font exercises successful receiver/install timing without
    /// depending on OS fonts. This is a Latin-metrics test, not CJK glyph proof.
    #[cfg(test)]
    pub(super) fn pending_success_for_test() -> (Self, impl FnOnce()) {
        let (tx, rx) = mpsc::channel();
        let deliver = move || {
            assert!(tx
                .send(Ok(LoadedFont {
                    bytes: CODE_FONT.to_vec(),
                    index: 0,
                    family: "Test fallback".into(),
                    path: "test-fixture.ttf".into(),
                }))
                .is_ok());
        };
        (
            Self {
                status: Status::Loading(rx),
            },
            deliver,
        )
    }

    pub fn needs_probe(&self) -> bool {
        matches!(self.status, Status::Unchecked)
    }
    /// Call only while the UI is running. File I/O and validation happen in a worker.
    pub fn tick(
        &mut self,
        ctx: &egui::Context,
        cjk_is_visible: bool,
    ) -> Option<Result<String, String>> {
        if self.needs_probe() && cjk_is_visible {
            let (tx, rx) = mpsc::channel();
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(load_system_font());
                repaint.request_repaint();
            });
            self.status = Status::Loading(rx);
        }
        let result = match &self.status {
            Status::Loading(rx) => match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The system font loader stopped".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            },
            _ => None,
        };
        result.map(|result| {
            self.status = Status::Finished;
            result.map(|font| {
                let label = format!("CJK fallback: {} ({})", font.family, font.path.display());
                install(ctx, font);
                label
            })
        })
    }
}

pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(|character| {
        matches!(character as u32,
        0x2e80..=0x303f | 0x3040..=0x30ff | 0x3100..=0x312f | 0x31a0..=0x31bf |
        0x31f0..=0x31ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7af |
        0xf900..=0xfaff | 0xff00..=0xffef | 0x20000..=0x323af)
    })
}
fn install(ctx: &egui::Context, font: LoadedFont) {
    ctx.set_fonts(with_system_fallback(font));
    ctx.request_repaint();
}

fn with_system_fallback(font: LoadedFont) -> egui::FontDefinitions {
    let mut definitions = base_fonts();
    let mut data = egui::FontData::from_owned(font.bytes);
    data.index = font.index;
    definitions
        .font_data
        .insert(FALLBACK_NAME.into(), Arc::new(data));
    // Preserve existing Latin/code fonts; use the system face only for missing glyphs.
    for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(FALLBACK_NAME.into());
    }
    definitions
}
fn candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let directory = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("Fonts");
        [
            "msyh.ttc",
            "msyh.ttf",
            "simsun.ttc",
            "YuGothR.ttc",
            "malgun.ttf",
        ]
        .iter()
        .map(|name| directory.join(name))
        .collect()
    }
    #[cfg(target_os = "macos")]
    {
        [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Supplemental/Songti.ttc",
            "/Library/Fonts/NotoSansCJK-Regular.ttc",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let mut paths: Vec<_> = [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/local/share/fonts/NotoSansCJK-Regular.ttc",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            paths.push(home.join(".local/share/fonts/NotoSansCJK-Regular.ttc"));
            paths.push(home.join(".fonts/NotoSansCJK-Regular.ttc"));
        }
        paths
    }
}
fn load_system_font() -> Result<LoadedFont, String> {
    for path in candidates() {
        if let Ok(font) = load_candidate(&path) {
            return Ok(font);
        }
    }
    Err("No supported installed CJK font was found within the 32 MiB limit. Chinese text is retained, but some characters may appear as boxes. Install a system CJK font and restart Cedar; no fonts were downloaded".into())
}
fn load_candidate(path: &Path) -> Result<LoadedFont, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_FONT_BYTES as u64 {
        return Err("Font is not a regular file within the size limit".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    (&mut file)
        .take(MAX_FONT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_FONT_BYTES {
        return Err("Font exceeds the size limit".into());
    }
    let (index, family) = choose_face(&bytes)?;
    Ok(LoadedFont {
        bytes,
        index,
        family,
        path: path.into(),
    })
}
fn choose_face(bytes: &[u8]) -> Result<(u32, String), String> {
    let count = ttf_parser::fonts_in_collection(bytes)
        .unwrap_or(1)
        .min(MAX_COLLECTION_FACES);
    let mut best: Option<(u32, i32, String)> = None;
    for index in 0..count {
        let Ok(face) = ttf_parser::Face::parse(bytes, index) else {
            continue;
        };
        let Ok(font) = ab_glyph::FontRef::try_from_slice_and_index(bytes, index) else {
            continue;
        };
        if !"中文你好世界"
            .chars()
            .all(|character| font.glyph_id(character).0 != 0)
        {
            continue;
        }
        let names: Vec<_> = face
            .names()
            .into_iter()
            .filter(|name| {
                name.name_id == ttf_parser::name_id::FAMILY
                    || name.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY
            })
            .filter_map(|name| name.to_string())
            .collect();
        let family = names
            .iter()
            .find(|name| name.is_ascii())
            .or_else(|| names.first())
            .cloned()
            .unwrap_or_else(|| "System CJK".into());
        let combined = names.join(" ");
        let score = if combined.contains("Mono CJK SC") {
            100
        } else if combined.contains("CJK SC") {
            90
        } else if combined.contains("PingFang SC") || combined.contains("Heiti SC") {
            80
        } else if combined.contains("YaHei") {
            70
        } else {
            0
        };
        if best.as_ref().is_none_or(|(_, current, _)| score > *current) {
            best = Some((index, score, family));
        }
    }
    best.map(|(index, _, family)| (index, family))
        .ok_or_else(|| "No parseable Chinese-capable face in this font".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_faces_match_pinned_identity_and_hashes() {
        use sha2::{Digest, Sha256};
        for (bytes, family, version, hash, monospaced) in [
            (
                UI_FONT,
                "Inter",
                "Version 4.001;git-9221beed3",
                "40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82",
                false,
            ),
            (
                CODE_FONT,
                "JetBrains Mono",
                "Version 2.304; ttfautohint (v1.8.4.7-5d5b)",
                "a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f",
                true,
            ),
        ] {
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), hash);
            let face = ttf_parser::Face::parse(bytes, 0).unwrap();
            let has_name = |id, expected: &str| {
                face.names()
                    .into_iter()
                    .any(|name| name.name_id == id && name.to_string().as_deref() == Some(expected))
            };
            assert!(has_name(ttf_parser::name_id::FAMILY, family));
            assert!(has_name(ttf_parser::name_id::SUBFAMILY, "Regular"));
            assert!(has_name(ttf_parser::name_id::VERSION, version));
            assert_eq!(face.weight().to_number(), 400);
            assert_eq!(face.is_monospaced(), monospaced);
            let font = ab_glyph::FontRef::try_from_slice(bytes).unwrap();
            assert!((32_u8..=126).all(|c| font.glyph_id(char::from(c)).0 != 0));
        }
    }

    #[test]
    fn base_fonts_are_repeatable_and_keep_builtin_fallbacks() {
        let base = base_fonts();
        assert_eq!(base, base_fonts());
        let defaults = egui::FontDefinitions::default();
        for (family, name) in [
            (egui::FontFamily::Proportional, UI_FONT_NAME),
            (egui::FontFamily::Monospace, CODE_FONT_NAME),
        ] {
            let fonts = &base.families[&family];
            assert_eq!(fonts[0], name);
            assert_eq!(&fonts[1..], defaults.families[&family]);
        }
        for (name, data) in defaults.font_data {
            assert_eq!(base.font_data[&name], data);
        }
    }

    #[test]
    fn kelvin_glyph_uses_embedded_inter_after_existing_monospace_fallbacks() {
        let base = base_fonts();
        let mono = &base.families[&egui::FontFamily::Monospace];
        let mut previous = vec![CODE_FONT_NAME.to_owned()];
        previous.extend(
            egui::FontDefinitions::default().families[&egui::FontFamily::Monospace].clone(),
        );
        // This is the exact pre-fix chain, not a host-dependent missing-font fixture.
        for name in &previous {
            let data = &base.font_data[name];
            let face = ttf_parser::Face::parse(&data.font, data.index).unwrap();
            assert!(
                face.glyph_index('K').is_none(),
                "{name} unexpectedly covers Kelvin"
            );
        }
        let inter = ttf_parser::Face::parse(UI_FONT, 0).unwrap();
        assert!(inter.glyph_index('K').is_some());
        assert_ne!(inter.glyph_index('K'), inter.glyph_index('K'));
        assert_eq!(
            mono,
            &previous
                .into_iter()
                .chain([UI_FONT_NAME.to_owned()])
                .collect::<Vec<_>>()
        );
        assert!(
            !contains_cjk("K K k Ω café 😀"),
            "Kelvin must not rely on an OS font probe"
        );
    }

    fn assert_kelvin_layout(ctx: &egui::Context, supported: bool) {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for size in [13.0, 14.0, 18.0] {
                let font = egui::FontId::monospace(size);
                ctx.fonts(|fonts| {
                    assert_eq!(fonts.has_glyph(&font, 'K'), supported);
                    assert!(!fonts.has_glyph(&font, '\u{10ffff}'));
                    let text = "K K k Ω café 😀";
                    let galley =
                        fonts.layout_no_wrap(text.into(), font.clone(), egui::Color32::WHITE);
                    assert_eq!(galley.job.text, text);
                    let glyph = galley.rows[0]
                        .glyphs
                        .iter()
                        .find(|glyph| glyph.chr == 'K')
                        .unwrap();
                    assert!(!glyph.uv_rect.is_nothing());
                    assert!(
                        glyph.uv_rect.size.is_finite()
                            && glyph.uv_rect.size.x > 0.0
                            && glyph.uv_rect.size.y > 0.0
                    );
                    let missing =
                        fonts.layout_no_wrap("\u{10ffff}".into(), font, egui::Color32::WHITE);
                    assert_eq!(
                        glyph.uv_rect == missing.rows[0].glyphs[0].uv_rect,
                        !supported
                    );
                    if supported {
                        // Same atlas, same existing Inter bytes, genuine U+212A glyph.
                        let reference = fonts.layout_no_wrap(
                            "K".into(),
                            egui::FontId::proportional(size),
                            egui::Color32::WHITE,
                        );
                        assert_eq!(glyph.uv_rect, reference.rows[0].glyphs[0].uv_rect);
                    }
                });
            }
        });
    }

    #[test]
    fn kelvin_layout_is_real_before_and_after_late_fallback_at_multiple_scales() {
        for scale in [1.0, 1.5, 2.0] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            let mut previous = base_fonts();
            previous
                .families
                .get_mut(&egui::FontFamily::Monospace)
                .unwrap()
                .retain(|name| name != UI_FONT_NAME);
            ctx.set_fonts(previous);
            assert_kelvin_layout(&ctx, false);
            ctx.set_fonts(base_fonts());
            assert_kelvin_layout(&ctx, true);
            for _ in 0..2 {
                install(&ctx, test_fallback());
                assert_kelvin_layout(&ctx, true);
            }
        }
    }

    #[test]
    fn embedded_missing_glyph_fallback_preserves_existing_text_metrics() {
        let ctx = egui::Context::default();
        let mut previous = base_fonts();
        previous
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .retain(|name| name != UI_FONT_NAME);
        ctx.set_fonts(previous);
        let before = latin_metrics(&ctx);
        ctx.set_fonts(base_fonts());
        assert_eq!(latin_metrics(&ctx), before);
    }

    fn test_fallback() -> LoadedFont {
        // This fixture exercises replacement ordering without depending on any
        // installed OS font. Actual CJK coverage is a separate optional test.
        LoadedFont {
            bytes: CODE_FONT.to_vec(),
            index: 0,
            family: "Test fallback".into(),
            path: "test-fixture.ttf".into(),
        }
    }

    #[test]
    fn late_fallback_only_appends_to_embedded_base() {
        let base = base_fonts();
        let mut with_fallback = with_system_fallback(test_fallback());
        assert!(with_fallback.font_data.remove(FALLBACK_NAME).is_some());
        for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
            let fonts = with_fallback.families.get_mut(&family).unwrap();
            assert_eq!(fonts.pop().as_deref(), Some(FALLBACK_NAME));
            assert!(!fonts.iter().any(|name| name == FALLBACK_NAME));
        }
        assert_eq!(with_fallback, base);
    }

    fn latin_metrics(ctx: &egui::Context) -> Vec<(egui::Vec2, Vec<egui::Pos2>)> {
        let mut metrics = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for font in [
                egui::FontId::proportional(13.0),
                egui::FontId::monospace(14.0),
            ] {
                let galley = ctx.fonts(|fonts| {
                    fonts.layout_no_wrap("Cedar 0123 Wi".into(), font, egui::Color32::WHITE)
                });
                metrics.push((
                    galley.size(),
                    galley.rows[0]
                        .glyphs
                        .iter()
                        .map(|glyph| glyph.pos)
                        .collect(),
                ));
            }
        });
        metrics
    }

    #[test]
    fn installing_late_fallback_does_not_reset_latin_metrics() {
        let ctx = egui::Context::default();
        ctx.set_fonts(base_fonts());
        let before = latin_metrics(&ctx);
        install(&ctx, test_fallback());
        assert_eq!(latin_metrics(&ctx), before);
    }

    #[test]
    fn detects_cjk_without_loading_for_ascii() {
        assert!(!contains_cjk("fn main() { println!(\"hello\"); }"));
        for text in ["中文注释", "你好，世界", "こんにちは", "안녕하세요"] {
            assert!(contains_cjk(text));
        }
    }
    #[test]
    fn malformed_and_oversized_fonts_are_rejected() {
        assert!(choose_face(b"not a font").is_err());
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(MAX_FONT_BYTES as u64 + 1).unwrap();
        assert!(load_candidate(file.path()).is_err());
    }
    #[test]
    #[ignore = "requires an installed supported system CJK font"]
    fn real_system_cjk_fallback_covers_chinese() {
        let ctx = egui::Context::default();
        ctx.set_fonts(base_fonts());
        let latin_before = latin_metrics(&ctx);
        let mut before = true;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            before =
                ctx.fonts(|fonts| fonts.has_glyphs(&egui::FontId::monospace(14.0), "中文你好世界"));
        });
        assert!(
            !before,
            "Pinned embedded Latin fonts unexpectedly already contain Chinese"
        );
        let font = load_system_font().expect("No supported system font");
        println!(
            "Loaded {} face {} from {} ({} bytes)",
            font.family,
            font.index,
            font.path.display(),
            font.bytes.len()
        );
        if font.path.to_string_lossy().contains("NotoSansCJK") {
            assert!(font.family.contains("SC"));
        }
        install(&ctx, font);
        let mut after = false;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            after = ctx.fonts(|fonts| {
                fonts.has_glyphs(
                    &egui::FontId::monospace(14.0),
                    "中文注释：远程工作区应保留 Unicode 文本。你好，世界",
                )
            });
        });
        assert!(
            after,
            "Installed font fallback did not make Chinese glyphs available to egui"
        );
        assert_eq!(latin_metrics(&ctx), latin_before);
    }
}
