//! Pure editor state: remote acknowledgements never replace a newer draft.
#[derive(Debug)]
pub struct Document {
    pub id: u64,
    pub path: String,
    pub text: String,
    pub saved_text: String,
    pub revision: Option<String>,
    pub saving: bool,
    pub interrupted_save: Option<crate::interrupted_save::InterruptedSave>,
    /// An unknown outcome without a trustworthy submission identity. Never
    /// manufacture a reconciliation token from the current (possibly newer) draft.
    pub save_outcome_unverifiable: bool,
    pub cursor: (usize, usize),
    pub jump_to: Option<usize>,
    pub scroll_to: Option<usize>,
    pub edit_version: u64,
    pub undo_initialized: bool,
    pub has_cjk: bool,
}

impl Document {
    pub fn new(id: u64, path: String, text: String, revision: String) -> Self {
        let has_cjk =
            crate::system_fonts::contains_cjk(&path) || crate::system_fonts::contains_cjk(&text);
        Self {
            id,
            path,
            saved_text: text.clone(),
            text,
            revision: Some(revision),
            saving: false,
            interrupted_save: None,
            save_outcome_unverifiable: false,
            cursor: (1, 1),
            jump_to: None,
            scroll_to: None,
            edit_version: 0,
            undo_initialized: false,
            has_cjk,
        }
    }
    pub fn dirty(&self) -> bool {
        self.save_outcome_unknown() || self.revision.is_none() || self.text != self.saved_text
    }
    pub fn save_outcome_unknown(&self) -> bool {
        self.interrupted_save.is_some() || self.save_outcome_unverifiable
    }
    pub fn acknowledge_save(&mut self, snapshot: String, revision: String) {
        self.saved_text = snapshot;
        self.revision = Some(revision);
        self.saving = false;
        self.interrupted_save = None;
    }
    /// Called only after a reviewed, twice-read clean reload passes its final
    /// frame guard. This is baseline adoption, never a write acknowledgement.
    pub fn adopt_reviewed_disk_baseline(&mut self, revision: String) {
        self.saved_text = self.text.clone();
        self.revision = Some(revision);
    }
}

/// egui cursor positions count Unicode scalar values, not UTF-8 bytes.
pub fn line_start(text: &str, target: usize) -> usize {
    if target <= 1 {
        return 0;
    }
    let mut line = 1;
    for (index, character) in text.chars().enumerate() {
        if character == '\n' {
            line += 1;
            if line == target {
                return index + 1;
            }
        }
    }
    text.chars().count()
}

pub fn cursor_location(text: &str, index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for character in text.chars().take(index) {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

pub const MAX_FIND_MATCHES: usize = 10_000;

pub fn find_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut byte_cursor = 0;
    let mut char_cursor = 0;
    let query_chars = query.chars().count();
    text.match_indices(query)
        .take(MAX_FIND_MATCHES)
        .map(|(byte, _)| {
            char_cursor += text[byte_cursor..byte].chars().count();
            let start = char_cursor;
            char_cursor += query_chars;
            byte_cursor = byte + query.len();
            start..char_cursor
        })
        .collect()
}

pub fn parent_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_owned())
        .unwrap_or_default()
}

pub fn language(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "rs" => "Rust",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "toml" => "TOML",
        "json" => "JSON",
        "md" => "Markdown",
        "yml" | "yaml" => "YAML",
        "xml" => "XML",
        "sh" => "Shell",
        "py" => "Python",
        "js" | "jsx" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        _ => "Plain text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_acknowledgement_keeps_newer_typing() {
        let mut doc = Document::new(1, "file.rs".into(), "old".into(), "r0".into());
        doc.text = "sent".into();
        doc.saving = true;
        let snapshot = doc.text.clone();
        doc.text = "newer draft".into();
        doc.acknowledge_save(snapshot, "r1".into());
        assert_eq!(doc.text, "newer draft");
        assert_eq!(doc.saved_text, "sent");
        assert_eq!(doc.revision.as_deref(), Some("r1"));
        assert!(doc.dirty());
    }
    #[test]
    fn exact_save_clears_dirty() {
        let mut doc = Document::new(1, "file".into(), "old".into(), "r0".into());
        doc.text = "new".into();
        doc.acknowledge_save("new".into(), "r1".into());
        assert!(!doc.dirty());
    }
    #[test]
    fn unicode_search_and_line_offsets_are_character_based() {
        let text = "é🐻\n你好 Rust\nend";
        assert_eq!(line_start(text, 2), 3);
        assert_eq!(find_ranges(text, "你好"), vec![3..5]);
        assert_eq!(cursor_location(text, 6), (2, 4));
        assert_eq!(line_start(text, 99), text.chars().count());
        assert!(find_ranges(text, "").is_empty());
    }
    #[test]
    fn dense_unicode_search_is_bounded() {
        let source = "é🐻".repeat(200_000);
        let ranges = find_ranges(&source, "🐻");
        assert_eq!(ranges.len(), MAX_FIND_MATCHES);
        assert_eq!(ranges[0], 1..2);
        assert_eq!(ranges[MAX_FIND_MATCHES - 1], 19_999..20_000);
    }
    #[test]
    fn root_parent_is_root() {
        assert_eq!(parent_path(""), "");
        assert_eq!(parent_path("src"), "");
        assert_eq!(parent_path("src/nested"), "src");
    }
}
