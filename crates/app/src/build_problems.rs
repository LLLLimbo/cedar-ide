//! Inert, bounded extraction of one English javac output grammar:
//! `path.java:positive-line: error|warning: message`.
//!
//! Lines are one-based decimal integers in 1..=u32::MAX, without leading zeros.
//! The path and message are literal text: no trimming, URL decoding, column
//! inference, tool detection, project discovery, or continuation-line parsing.
//! stdout precedes stderr; rows within each stream retain their original order.
//! Only strict workspace-relative paths get a possible Read target. The task's
//! backend OS, never the frontend OS, determines separator interpretation. The
//! backend's ordinary Read remains the root/symlink/file authority.

pub(super) const MAX_ROWS: usize = 256;
pub(super) const MAX_PATH_BYTES: usize = 4096;
pub(super) const MAX_MESSAGE_BYTES: usize = 2048;
pub(super) const MAX_RETAINED_BYTES: usize = 512 * 1024;
// Task snapshots permit 3x the byte capture limit after lossy UTF-8 decoding.
// Keep this independent cap even when called with arbitrary input strings.
pub(super) const MAX_SCAN_BYTES_PER_STREAM: usize = 768 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BuildProblem {
    pub path: String,
    pub line: usize,
    pub severity: Severity,
    pub stream: Stream,
    pub output_line: usize,
    pub message: String,
    pub message_truncated: bool,
    pub navigation_path: Option<String>,
    pub disabled_reason: Option<&'static str>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ParseSummary {
    /// Complete lines outside the supported grammar (including source/carets).
    pub ignored_lines: usize,
    /// Location-like lines rejected as malformed, or omitted by a display cap.
    pub skipped_lines: usize,
    /// Retained rows whose displayed message is only an explicitly marked prefix.
    pub truncated_messages: usize,
    /// Input bytes not parsed, including an incomplete line at a scan boundary.
    pub unscanned_bytes: usize,
    /// All owned path, navigation-path, and message bytes, excluding Vec metadata.
    pub retained_bytes: usize,
    pub row_limit_reached: bool,
    pub text_limit_reached: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct BuildProblems {
    pub rows: Vec<BuildProblem>,
    pub summary: ParseSummary,
}

pub(super) fn parse_javac_locations(stdout: &str, stderr: &str, backend_os: &str) -> BuildProblems {
    let mut result = BuildProblems::default();
    parse_stream(stdout, Stream::Stdout, backend_os, &mut result);
    parse_stream(stderr, Stream::Stderr, backend_os, &mut result);
    result
}

fn parse_stream(input: &str, stream: Stream, backend_os: &str, result: &mut BuildProblems) {
    let end = if input.len() <= MAX_SCAN_BYTES_PER_STREAM {
        input.len()
    } else {
        // LF is an ASCII boundary, so this does not split UTF-8 or create a
        // seemingly complete diagnostic from a cut-off line/message.
        input.as_bytes()[..MAX_SCAN_BYTES_PER_STREAM]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |position| position + 1)
    };
    result.summary.unscanned_bytes = result
        .summary
        .unscanned_bytes
        .saturating_add(input.len() - end);
    for (index, raw) in input[..end].split_inclusive('\n').enumerate() {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        // A final bare CR is not a CRLF terminator and must remain invalid.
        let line = if raw.ends_with('\n') {
            line.strip_suffix('\r').unwrap_or(line)
        } else {
            line
        };
        parse_line(line, stream, index + 1, backend_os, result);
    }
}

fn parse_line(
    text: &str,
    stream: Stream,
    output_line: usize,
    backend_os: &str,
    result: &mut BuildProblems,
) {
    if !text.contains(".java:") {
        result.summary.ignored_lines += 1;
        return;
    }
    let Some((path, line, severity, message)) = location_parts(text) else {
        result.summary.skipped_lines += 1;
        return;
    };
    if path.len() > MAX_PATH_BYTES || text.chars().any(forbidden_control) {
        // Never truncate a path or line into a different, navigable location.
        result.summary.skipped_lines += 1;
        return;
    }
    if result.rows.len() == MAX_ROWS {
        result.summary.row_limit_reached = true;
        result.summary.skipped_lines += 1;
        return;
    }
    let disabled_reason = navigation_problem(path, backend_os);
    let message_end = utf8_prefix_end(message, MAX_MESSAGE_BYTES);
    let message_truncated = message_end < message.len();
    let retained_bytes = path.len()
        + if disabled_reason.is_none() {
            path.len()
        } else {
            0
        }
        + message_end;
    if result.summary.retained_bytes + retained_bytes > MAX_RETAINED_BYTES {
        result.summary.text_limit_reached = true;
        result.summary.skipped_lines += 1;
        return;
    }
    result.summary.retained_bytes += retained_bytes;
    result.summary.truncated_messages += usize::from(message_truncated);
    result.rows.push(BuildProblem {
        path: path.to_owned(),
        line,
        severity,
        stream,
        output_line,
        message: message[..message_end].to_owned(),
        message_truncated,
        navigation_path: disabled_reason.is_none().then(|| {
            if backend_os == "windows" {
                path.replace('\\', "/")
            } else {
                path.to_owned()
            }
        }),
        disabled_reason,
    });
}

fn location_parts(text: &str) -> Option<(&str, usize, Severity, &str)> {
    // Two fixed-string searches, followed by a single right split, keep this
    // linear even with repeated colons or fake diagnostics inside a message.
    let error = text.find(": error: ").map(|at| (at, Severity::Error));
    let warning = text.find(": warning: ").map(|at| (at, Severity::Warning));
    let (at, severity) = match (error, warning) {
        (Some(error), Some(warning)) => {
            if error.0 < warning.0 {
                error
            } else {
                warning
            }
        }
        (Some(found), None) | (None, Some(found)) => found,
        (None, None) => return None,
    };
    let (path, number) = text[..at].rsplit_once(':')?;
    if path.is_empty()
        || !path.ends_with(".java")
        || number.is_empty()
        || number.starts_with('0')
        || !number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let line = usize::try_from(number.parse::<u32>().ok()?).ok()?;
    let message = &text[at + severity.label().len() + 4..];
    if message.is_empty() {
        return None;
    }
    Some((path, line, severity, message))
}

fn utf8_prefix_end(text: &str, limit: usize) -> usize {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn forbidden_control(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn navigation_problem(path: &str, backend_os: &str) -> Option<&'static str> {
    let windows = match backend_os {
        "windows" => true,
        "linux" | "macos" | "freebsd" | "openbsd" | "netbsd" | "dragonfly" | "solaris"
        | "illumos" | "aix" | "android" => false,
        _ => return Some("The task's backend path syntax is unknown"),
    };
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return Some("Only workspace-relative paths are supported; no absolute paths, URLs, drives, devices, or UNC paths");
    }
    if path.contains('\u{fffd}') {
        // Task capture uses lossy decoding. U+FFFD could be a real filename or
        // replacement for bytes that no longer identify the original path.
        return Some("Replacement characters make the output path ambiguous");
    }
    if !windows && path.contains('\\') {
        return Some("A literal backslash cannot be represented by the workspace Read path");
    }
    for component in path.split(|character| character == '/' || (windows && character == '\\')) {
        if component.is_empty() || matches!(component, "." | "..") {
            return Some("Empty, dot, and parent path components are not supported");
        }
        if windows
            && (component.ends_with(['.', ' '])
                || component.contains(['<', '>', '"', '|', '?', '*'])
                || windows_device_name(component))
        {
            return Some("The path contains an ambiguous or reserved Windows name");
        }
    }
    None
}

fn windows_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ');
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(name))
    {
        return true;
    }
    let Some(prefix) = stem.get(..3) else {
        return false;
    };
    (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
        && matches!(
            &stem[3..],
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str, os: &str) -> BuildProblems {
        parse_javac_locations(text, "", os)
    }

    #[test]
    fn unicode_spaces_crlf_and_stream_identity_are_preserved() {
        let result = parse_javac_locations(
            "src/中文 空格.java:2: error: 找不到符号\r\n  source\r\n  ^\r\n",
            "src/中文 空格.java:12: warning: [unchecked] literal %20\n1 warning\n",
            "linux",
        );
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.summary.ignored_lines, 3);
        assert_eq!(result.summary.skipped_lines, 0);
        assert_eq!(result.rows[0].stream, Stream::Stdout);
        assert_eq!(result.rows[1].stream, Stream::Stderr);
        assert_eq!(result.rows[0].output_line, 1);
        assert_eq!(result.rows[1].output_line, 1);
        assert_eq!(result.rows[0].line, 2);
        assert_eq!(result.rows[1].line, 12);
        assert_eq!(result.rows[0].severity, Severity::Error);
        assert_eq!(result.rows[1].severity, Severity::Warning);
        assert_eq!(result.rows[0].message, "找不到符号");
        assert_eq!(result.rows[1].message, "[unchecked] literal %20");
        assert_eq!(
            result.rows[0].navigation_path.as_deref(),
            Some("src/中文 空格.java")
        );
    }

    #[test]
    fn independent_streams_never_form_a_synthetic_diagnostic() {
        let result = parse_javac_locations("src/Main.java:1: error:", " message", "linux");
        assert!(result.rows.is_empty());
        assert_eq!(result.summary.skipped_lines, 1);
        assert_eq!(result.summary.ignored_lines, 1);
    }

    #[test]
    fn deterministic_order_keeps_duplicates_and_original_output_lines() {
        let stdout = "header\nMain.java:3: error: again\nMain.java:3: error: again\n";
        let stderr = "\nMain.java:1: warning: again\n";
        let result = parse_javac_locations(stdout, stderr, "linux");
        assert_eq!(result, parse_javac_locations(stdout, stderr, "linux"));
        assert_eq!(result.rows.len(), 3);
        assert_eq!(
            result
                .rows
                .iter()
                .map(|row| row.output_line)
                .collect::<Vec<_>>(),
            vec![2, 3, 2]
        );
        assert_eq!(
            result.rows.iter().map(|row| row.line).collect::<Vec<_>>(),
            vec![3, 3, 1]
        );
        assert_eq!(result.rows[2].stream.label(), "stderr");
        assert_eq!(result.rows[0].stream.label(), "stdout");
    }

    #[test]
    fn exact_grammar_rejects_malformed_numbers_and_does_not_infer_columns() {
        for text in [
            "Main.java:0: error: zero",
            "Main.java:01: error: leading zero",
            "Main.java:-1: error: negative",
            "Main.java:+1: error: signed",
            "Main.java: 1: error: whitespace",
            "Main.java:1 : error: whitespace",
            "Main.java:١: error: nonascii",
            "Main.java:4294967296: error: overflow",
            "Main.java:184467440737095516160000: error: overflow",
            "Main.java:1:2: error: column",
            "Main.java:1:error: missing space",
            "Main.java:1: Error: capitalized",
            "Main.java:1: note: unsupported severity",
            "Main.java:1: error: ",
            "Main.java:1: error:",
        ] {
            let result = parse(text, "linux");
            assert!(result.rows.is_empty(), "{text}");
            assert_eq!(result.summary.skipped_lines, 1, "{text}");
        }
        let result = parse("Main.java:4294967295: warning: exact upper bound", "linux");
        assert_eq!(result.rows[0].line, u32::MAX as usize);
        assert!(parse("at example.Main.call(Main.java:10)", "linux")
            .rows
            .is_empty());
        assert!(parse("Main.kt:1: error: not Java", "linux").rows.is_empty());
    }

    #[test]
    fn controls_and_bare_cr_reject_whole_locations_including_message_suffixes() {
        for control in [
            '\0', '\t', '\r', '\u{1b}', '\u{7f}', '\u{85}', '\u{061c}', '\u{200e}', '\u{200f}',
            '\u{2028}', '\u{2029}', '\u{202e}', '\u{2067}',
        ] {
            for text in [
                format!("src/{control}Main.java:1: error: message"),
                format!("Main.java:1: error: mes{control}sage"),
                format!(
                    "Main.java:1: error: {}{control}",
                    "x".repeat(MAX_MESSAGE_BYTES)
                ),
            ] {
                let result = parse(&text, "linux");
                assert!(result.rows.is_empty(), "{text:?}");
                assert_eq!(result.summary.skipped_lines, 1);
            }
        }
        assert!(parse("Main.java:1: error: message\r", "linux")
            .rows
            .is_empty());
        assert_eq!(
            parse("Main.java:1: error: message\r\n", "linux").rows.len(),
            1
        );
    }

    #[test]
    fn unsafe_paths_stay_visible_with_no_navigation_permission() {
        for os in ["linux", "windows"] {
            for path in [
                "/workspace/Main.java",
                "/workspace-other/Main.java",
                "C:/workspace/Main.java",
                "C:\\workspace\\Main.java",
                "C:Main.java",
                "file:///workspace/Main.java",
                "https://host/Main.java",
                "//server/share/Main.java",
                "\\\\server\\share\\Main.java",
                "\\\\?\\C:\\workspace\\Main.java",
                "\\\\.\\device\\Main.java",
                "\\??\\C:\\workspace\\Main.java",
                "/Device/HarddiskVolume1/Main.java",
                "../Main.java",
                "src/../Main.java",
                "./Main.java",
                "src/./Main.java",
                "src//Main.java",
                "src:Main.java",
                "src/\u{fffd}Main.java",
            ] {
                let result = parse(&format!("{path}:1: error: message"), os);
                assert_eq!(result.rows.len(), 1, "{os} {path}");
                let row = &result.rows[0];
                assert_eq!(row.path, path);
                assert!(row.navigation_path.is_none(), "{os} {path}");
                assert!(row.disabled_reason.is_some(), "{os} {path}");
            }
        }
    }

    #[test]
    fn windows_separators_depend_only_on_backend_os() {
        let text = "src\\中文 空格\\Main.java:1: error: message";
        assert_eq!(
            parse(text, "windows").rows[0].navigation_path.as_deref(),
            Some("src/中文 空格/Main.java")
        );
        assert!(parse(text, "linux").rows[0].navigation_path.is_none());
        assert!(parse("Main.java:1: error: message", "").rows[0]
            .navigation_path
            .is_none());
        assert!(parse("Main.java:1: error: message", "unknown").rows[0]
            .navigation_path
            .is_none());
        for path in [
            "src\\..\\Main.java",
            "src\\.\\Main.java",
            "src\\\\Main.java",
            "\\Main.java",
        ] {
            assert!(
                parse(&format!("{path}:1: error: message"), "windows").rows[0]
                    .navigation_path
                    .is_none(),
                "{path}"
            );
        }
    }

    #[test]
    fn literal_names_are_never_trimmed_case_folded_or_percent_decoded() {
        for path in [
            " 中文 空格.java",
            "目录 /Main.java",
            "%2e%2e/Main.java",
            "%2Froot/Main.java",
            "%5cserver/Main.java",
            "%00.java",
            "%ZZ.java",
            "MiXeD.java",
        ] {
            let result = parse(&format!("{path}:1: error: message  "), "linux");
            assert_eq!(result.rows[0].path, path);
            assert_eq!(result.rows[0].navigation_path.as_deref(), Some(path));
            assert_eq!(result.rows[0].message, "message  ");
        }
        // POSIX names that Windows would interpret specially remain literal.
        for path in [
            "NUL.java",
            "dir./Main.java",
            "src/?name.java",
            "src/quote\".java",
        ] {
            assert_eq!(
                parse(&format!("{path}:1: error: message"), "linux").rows[0]
                    .navigation_path
                    .as_deref(),
                Some(path)
            );
        }
    }

    #[test]
    fn windows_alias_and_device_names_never_become_read_targets() {
        for path in [
            "CON.java",
            "nul.java",
            "PrN.java",
            "AUX.java",
            "CONIN$.java",
            "CONOUT$.java",
            "COM1.java",
            "lpt9.java",
            "COM¹.java",
            "LPT².java",
            "COM³.java",
            "CON .java",
            "dir./Main.java",
            "dir /Main.java",
            "src/?Main.java",
            "src/*Main.java",
            "src/<Main.java",
            "src/>Main.java",
            "src/|Main.java",
            "src/\"Main.java",
        ] {
            let result = parse(&format!("{path}:1: error: message"), "windows");
            assert_eq!(result.rows.len(), 1, "{path}");
            assert!(result.rows[0].navigation_path.is_none(), "{path}");
        }
        for path in ["COM0.java", "COM10.java", "CONSOLE.java", "中文.java"] {
            assert_eq!(
                parse(&format!("{path}:1: error: message"), "windows").rows[0]
                    .navigation_path
                    .as_deref(),
                Some(path)
            );
        }
    }

    #[test]
    fn message_severity_words_and_colons_do_not_relabel_locations() {
        let result = parse(
            "Main.java:7: warning: value: error: Other.java:8: error: nested",
            "linux",
        );
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].severity, Severity::Warning);
        assert_eq!(result.rows[0].line, 7);
        assert_eq!(
            result.rows[0].message,
            "value: error: Other.java:8: error: nested"
        );
        assert!(
            parse("Main.java:0: error: Other.java:1: error: nested", "linux")
                .rows
                .is_empty()
        );
    }

    #[test]
    fn path_limit_is_exact_and_oversized_paths_are_never_shortened() {
        let at_limit = format!("{}.java", "x".repeat(MAX_PATH_BYTES - 5));
        let result = parse(&format!("{at_limit}:1: error: message"), "linux");
        assert_eq!(result.rows[0].path, at_limit);
        assert_eq!(
            result.rows[0].navigation_path.as_deref(),
            Some(at_limit.as_str())
        );
        let result = parse(&format!("x{at_limit}:1: error: message"), "linux");
        assert!(result.rows.is_empty());
        assert_eq!(result.summary.skipped_lines, 1);
    }

    #[test]
    fn message_prefixes_are_utf8_bounded_and_explicitly_marked() {
        let message = "x".repeat(MAX_MESSAGE_BYTES);
        let result = parse(&format!("Main.java:1: error: {message}"), "linux");
        assert_eq!(result.rows[0].message, message);
        assert!(!result.rows[0].message_truncated);
        let message = format!("{}雪suffix", "x".repeat(MAX_MESSAGE_BYTES - 1));
        let result = parse(&format!("Main.java:1: error: {message}"), "linux");
        assert_eq!(result.rows[0].message.len(), MAX_MESSAGE_BYTES - 1);
        assert!(result.rows[0].message_truncated);
        assert_eq!(result.summary.truncated_messages, 1);
        assert_eq!(result.rows[0].line, 1);
        assert_eq!(result.rows[0].navigation_path.as_deref(), Some("Main.java"));
    }

    #[test]
    fn row_cap_counts_omitted_locations_across_both_streams() {
        let line = "Main.java:1: error: message\n";
        let stdout = line.repeat(MAX_ROWS);
        let result = parse_javac_locations(&stdout, line, "linux");
        assert_eq!(result.rows.len(), MAX_ROWS);
        assert!(result.summary.row_limit_reached);
        assert_eq!(result.summary.skipped_lines, 1);
        assert_eq!(result.summary.unscanned_bytes, 0);
        assert!(result.rows.iter().all(|row| row.stream == Stream::Stdout));
        assert!(!parse(&stdout, "linux").summary.row_limit_reached);
    }

    #[test]
    fn retained_text_cap_includes_duplicate_navigation_path_storage() {
        let path = format!("{}.java", "x".repeat(MAX_PATH_BYTES - 5));
        let message = "m".repeat(MAX_MESSAGE_BYTES);
        let line = format!("{path}:1: error: {message}\n");
        let result = parse(&line.repeat(100), "linux");
        let per_row = MAX_PATH_BYTES * 2 + MAX_MESSAGE_BYTES;
        assert_eq!(result.rows.len(), MAX_RETAINED_BYTES / per_row);
        assert_eq!(result.summary.retained_bytes, result.rows.len() * per_row);
        assert!(result.summary.retained_bytes <= MAX_RETAINED_BYTES);
        assert!(result.summary.text_limit_reached);
        assert_eq!(result.summary.skipped_lines, 100 - result.rows.len());
        assert_eq!(result.summary.unscanned_bytes, 0);
        // Later smaller rows may still fit: do not silently abandon the stream.
        let result = parse_javac_locations(&line.repeat(100), "A.java:1: error: x", "linux");
        assert_eq!(result.rows.last().unwrap().stream, Stream::Stderr);
        assert!(result.summary.retained_bytes <= MAX_RETAINED_BYTES);

        let path = format!("{}.java", "x".repeat(2047 - 5));
        let line = format!("{path}:1: error: xx\n");
        let exact = line.repeat(128);
        let result = parse(&exact, "linux");
        assert_eq!(result.summary.retained_bytes, MAX_RETAINED_BYTES);
        assert_eq!(result.rows.len(), 128);
        assert!(!result.summary.text_limit_reached);
        let result = parse_javac_locations(&exact, "A.java:1: error: x", "linux");
        assert_eq!(result.rows.len(), 128);
        assert!(result.summary.text_limit_reached);
        assert_eq!(result.summary.skipped_lines, 1);
    }

    #[test]
    fn scan_cap_omits_partial_lines_and_reports_exact_unparsed_bytes() {
        let first = "Main.java:1: error: complete\n";
        let prefix = "x".repeat(MAX_SCAN_BYTES_PER_STREAM - first.len());
        let input = format!("{first}{prefix}雪\nOther.java:2: warning: outside\n");
        let result = parse_javac_locations(&input, "stderr.java:3: error: retained", "linux");
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[1].stream, Stream::Stderr);
        assert_eq!(result.summary.unscanned_bytes, input.len() - first.len());
        assert_eq!(result.summary.skipped_lines, 0);
        let exact = format!("{}\n", "x".repeat(MAX_SCAN_BYTES_PER_STREAM - 1));
        assert_eq!(parse(&exact, "linux").summary.unscanned_bytes, 0);
        let cut = format!("{}雪", "x".repeat(MAX_SCAN_BYTES_PER_STREAM - 1));
        let result = parse(&cut, "linux");
        assert!(result.rows.is_empty());
        assert_eq!(result.summary.unscanned_bytes, cut.len());
        let result = parse_javac_locations(&cut, &cut, "linux");
        assert_eq!(result.summary.unscanned_bytes, cut.len() * 2);
    }
}
