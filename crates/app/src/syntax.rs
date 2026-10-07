//! Small lexical colorizer; not a parser or language server.
use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};
const TEXT: Color32 = Color32::from_rgb(211, 219, 231);
const COMMENT: Color32 = Color32::from_rgb(116, 132, 145);
const STRING: Color32 = Color32::from_rgb(167, 210, 151);
const KEYWORD: Color32 = Color32::from_rgb(190, 166, 237);
const NUMBER: Color32 = Color32::from_rgb(230, 190, 132);

pub fn highlight(text: &str, font_size: f32, enabled: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let format = |color| TextFormat {
        font_id: FontId::monospace(font_size),
        color,
        ..Default::default()
    };
    if !enabled || text.len() > 256 * 1024 {
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

#[cfg(test)]
mod tests {
    use super::*;
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
