//! Small lexical colorizer; not a parser or language server.
use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};
// JetBrains Islands Dark, IslandSchemeDark.xml at
// e7e7972a41e572f9179f742685a9d9f739a7943b. Existing lexical classes only;
// this does not imply IDEA's parsed/resolved semantic highlighting.
const TEXT: Color32 = Color32::from_rgb(188, 190, 196);
const GUTTER: Color32 = Color32::from_rgb(111, 115, 122);
const COMMENT: Color32 = Color32::from_rgb(122, 126, 133);
const STRING: Color32 = Color32::from_rgb(106, 171, 115);
const KEYWORD: Color32 = Color32::from_rgb(207, 142, 109);
const NUMBER: Color32 = Color32::from_rgb(42, 172, 184);

/// Both code and its smaller line numbers use this explicit logical row height.
/// egui snaps it to physical pixels, identically for both galleys at every scale.
pub fn editor_line_height(code_size: f32) -> f32 {
    code_size * 1.2
}

fn editor_format(font_size: f32, code_size: f32, color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::monospace(font_size),
        line_height: Some(editor_line_height(code_size)),
        color,
        ..Default::default()
    }
}

pub fn highlight(text: &str, font_size: f32, enabled: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let format = |color| editor_format(font_size, font_size, color);
    if text.is_empty() || !enabled || text.len() > 256 * 1024 {
        job.append(text, 0.0, format(TEXT));
        return job;
    }
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        let (length, color) = if rest.starts_with("//") {
            (rest.find('\n').unwrap_or(rest.len()), COMMENT)
        } else if let Some(comment) = rest.strip_prefix("/*") {
            (
                comment.find("*/").map(|n| n + 4).unwrap_or(rest.len()),
                COMMENT,
            )
        } else if rest.starts_with('"') || rest.starts_with('\'') {
            let quote = rest.as_bytes()[0];
            let mut end = 1;
            let mut escaped = false;
            for ch in rest[1..].chars() {
                end += ch.len_utf8();
                if ch as u32 == u32::from(quote) && !escaped {
                    break;
                }
                if ch == '\n' && !escaped {
                    break;
                }
                escaped = ch == '\\' && !escaped;
            }
            (end, STRING)
        } else {
            let ch = rest.chars().next().unwrap_or(' ');
            if ch.is_alphabetic() || ch == '_' {
                let end = rest
                    .char_indices()
                    .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                    .map(|(i, _)| i)
                    .unwrap_or(rest.len());
                let token = &rest[..end];
                let keyword = matches!(
                    token,
                    "fn" | "let"
                        | "mut"
                        | "pub"
                        | "struct"
                        | "enum"
                        | "impl"
                        | "use"
                        | "mod"
                        | "trait"
                        | "where"
                        | "match"
                        | "if"
                        | "else"
                        | "for"
                        | "in"
                        | "while"
                        | "loop"
                        | "return"
                        | "break"
                        | "continue"
                        | "async"
                        | "await"
                        | "move"
                        | "const"
                        | "static"
                        | "self"
                        | "Self"
                        | "true"
                        | "false"
                        | "class"
                        | "public"
                        | "private"
                        | "protected"
                        | "final"
                        | "void"
                        | "new"
                        | "null"
                        | "this"
                        | "super"
                        | "import"
                        | "package"
                        | "extends"
                        | "implements"
                        | "try"
                        | "catch"
                        | "throw"
                        | "throws"
                        | "interface"
                        | "val"
                        | "var"
                        | "fun"
                        | "when"
                        | "object"
                        | "data"
                        | "override"
                        | "suspend"
                        | "internal"
                        | "open"
                        | "is"
                        | "as"
                        | "type"
                        | "unsafe"
                        | "extern"
                        | "ref"
                        | "dyn"
                );
                (end, if keyword { KEYWORD } else { TEXT })
            } else if ch.is_ascii_digit() {
                let end = rest
                    .char_indices()
                    .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '.' && *c != '_')
                    .map(|(i, _)| i)
                    .unwrap_or(rest.len());
                (end, NUMBER)
            } else {
                (ch.len_utf8(), TEXT)
            }
        };
        job.append(&rest[..length], 0.0, format(color));
        index += length;
    }
    job
}

pub fn supports(path: &str) -> bool {
    matches!(path.rsplit('.').next(), Some("rs" | "java" | "kt" | "kts"))
}

pub fn galley(ui: &egui::Ui, text: &str, size: f32, enabled: bool) -> std::sync::Arc<egui::Galley> {
    ui.fonts(|fonts| fonts.layout_job(highlight(text, size, enabled)))
}

fn gutter_job(text: &str, code_size: f32) -> LayoutJob {
    LayoutJob::single_section(
        text.into(),
        editor_format((code_size - 1.0).max(1.0), code_size, GUTTER),
    )
}

fn gutter_layout(
    fonts: &egui::text::Fonts,
    text: &str,
    code_size: f32,
) -> (std::sync::Arc<egui::Galley>, f32) {
    let gutter = fonts.layout_job(gutter_job(text, code_size));
    // An equal line height prevents cumulative drift, but the 13 pt face still
    // has a different ascent from 14 pt code. Measure the actual rasterized
    // baselines so the offset also follows UI scale and font changes.
    let code_sample = fonts.layout_job(highlight("0", code_size, false));
    let gutter_sample = fonts.layout_job(gutter_job("0", code_size));
    let baseline_offset =
        code_sample.rows[0].glyphs[0].pos.y - gutter_sample.rows[0].glyphs[0].pos.y;
    (gutter, baseline_offset)
}

/// Noninteractive line numbers, aligned to a code galley with a zero top margin.
pub fn gutter(ui: &mut egui::Ui, line_numbers: &str, code_size: f32) -> egui::Response {
    let (galley, baseline_offset) = ui.fonts(|fonts| gutter_layout(fonts, line_numbers, code_size));
    let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), line_numbers)
    });
    ui.painter()
        .galley(rect.min + egui::vec2(0.0, baseline_offset), galley, GUTTER);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_and_smaller_gutter_request_the_same_line_height() {
        for code_size in [12.0, 14.0, 20.0] {
            for source in ["", "\n", "class Example { int n = 123; }\n// 中文"] {
                for enabled in [false, true] {
                    let code = highlight(source, code_size, enabled);
                    assert!(!code.sections.is_empty());
                    for section in code.sections {
                        assert_eq!(section.format.font_id.size, code_size);
                        assert_eq!(
                            section.format.line_height,
                            Some(editor_line_height(code_size))
                        );
                    }
                }
            }
            let gutter = gutter_job("  1\n100\n200", code_size);
            assert_eq!(gutter.sections[0].format.font_id.size, code_size - 1.0);
            assert_eq!(
                gutter.sections[0].format.line_height,
                Some(editor_line_height(code_size))
            );
        }
        assert!((editor_line_height(14.0) - 16.8).abs() < 0.0001);
    }

    #[test]
    fn code_and_gutter_baselines_stay_aligned_through_line_200_at_multiple_scales() {
        let line_numbers = (1..=200)
            .map(|line| format!("{line:>3}"))
            .collect::<Vec<_>>()
            .join("\n");
        for pixels_per_point in [1.0, 1.25, 1.5, 2.0] {
            for code_size in [12.0, 14.0, 20.0] {
                let ctx = egui::Context::default();
                ctx.set_fonts(crate::system_fonts::base_fonts());
                ctx.set_pixels_per_point(pixels_per_point);
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    ctx.fonts(|fonts| {
                        let (numbers, baseline_offset) =
                            gutter_layout(fonts, &line_numbers, code_size);
                        for (source, enabled) in [
                            ("Plain text 0123", false),
                            ("class Example { int value = 123; } // comment", true),
                        ] {
                            let text = std::iter::repeat_n(source, 200)
                                .collect::<Vec<_>>()
                                .join("\n");
                            let code = fonts.layout_job(highlight(&text, code_size, enabled));
                            assert_eq!(code.rows.len(), 200);
                            assert_eq!(numbers.rows.len(), 200);
                            for index in [0, 99, 199] {
                                let code_row = &code.rows[index];
                                let number_row = &numbers.rows[index];
                                assert_eq!(code_row.rect.min.y, number_row.rect.min.y);
                                assert_eq!(code_row.rect.height(), number_row.rect.height());
                                let code_baseline = code_row.glyphs[0].pos.y;
                                let number_baseline = number_row.glyphs[0].pos.y + baseline_offset;
                                assert!(
                                    (code_baseline - number_baseline).abs() < 0.05,
                                    "line {}, font {code_size}, scale {pixels_per_point}: code {code_baseline}, gutter {number_baseline}",
                                    index + 1,
                                );
                            }
                        }
                    });
                });
            }
        }
    }

    #[test]
    fn preserves_unicode_and_unclosed_strings() {
        for source in [
            "fn main() { // 你好\n let café = \"hi🐻\"; }",
            "\"unclosed",
            "/* unfinished",
            "",
            "'a'",
        ] {
            let job = highlight(source, 14.0, true);
            assert_eq!(job.text, source);
            assert!(job
                .sections
                .iter()
                .all(|s| source.is_char_boundary(s.byte_range.start)
                    && source.is_char_boundary(s.byte_range.end)));
        }
    }
}
