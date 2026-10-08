//! Display identity only: equivalent JDT spellings may name a known Java URI.
//! No filesystem resolution, path traversal, or navigation permission is granted.
use super::*;
use std::borrow::Cow;

impl CedarApp {
    pub(in crate::language_ui) fn normalize_known_java_diagnostic<'a>(
        &self,
        value: &'a Value,
    ) -> Cow<'a, Value> {
        let Some(actual) = value.get("uri").and_then(Value::as_str) else {
            return Cow::Borrowed(value);
        };
        let Some(canonical) = self.known_java_diagnostic_uri(actual) else {
            return Cow::Borrowed(value);
        };
        if canonical == actual {
            return Cow::Borrowed(value);
        }
        // Only an incoming bounded publication is copied, never editor text.
        let mut normalized = value.clone();
        normalized["uri"] = Value::String(canonical);
        Cow::Owned(normalized)
    }

    fn known_java_diagnostic_uri(&self, actual: &str) -> Option<String> {
        if self.language.mode != ServerMode::Java {
            return None;
        }
        let windows = self
            .agent_info
            .as_ref()
            .is_some_and(|info| info.os == "windows");
        let actual_path = local_uri_path(actual, windows)?;
        let opened = self
            .language
            .sync
            .opened
            .iter()
            .filter(|(id, _)| {
                self.documents
                    .iter()
                    .any(|doc| doc.id == **id && self.language.matches(&doc.path))
            })
            .map(|(_, ack)| &ack.uri);
        let mut matched: Option<&String> = None;
        for known in opened.chain(self.language.closed_uris.iter()) {
            if local_uri_path(known, windows).as_ref() == Some(&actual_path) {
                if matched.is_some_and(|previous| previous != known) {
                    return None;
                }
                matched = Some(known);
            }
        }
        matched.cloned()
    }
}

fn local_uri_path(uri: &str, windows: bool) -> Option<Vec<u8>> {
    if uri.len() > language_results::MAX_DIAGNOSTIC_URI_BYTES {
        return None;
    }
    let path = uri.strip_prefix("file:")?;
    let path = path.strip_prefix("//").unwrap_or(path);
    if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#', '\\']) {
        return None;
    }
    let mut decoded = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            let byte = ((high << 4) | low) as u8;
            if matches!(byte, b'/' | b'\\') {
                return None;
            }
            byte
        } else {
            byte
        };
        if byte.is_ascii_control() {
            return None;
        }
        decoded.push(byte);
    }
    let text = std::str::from_utf8(&decoded).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    if text[1..]
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    if windows {
        if decoded.len() < 5 || !decoded[1].is_ascii_alphabetic() || decoded[2..4] != *b":/" {
            return None;
        }
        for part in text[4..].split('/') {
            if part.ends_with(['.', ' ']) || part.contains([':', '<', '>', '|', '?', '*']) {
                return None;
            }
            let stem = part.split('.').next()?.to_ascii_uppercase();
            if matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ) || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|suffix| {
                    matches!(
                        suffix,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            }) {
                return None;
            }
        }
        decoded[1].make_ascii_uppercase();
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_java_uri_identity_is_strict_local_utf8_and_only_folds_windows_drive() {
        assert_eq!(
            local_uri_path("file:/C:/work%20雪/Main.java", true),
            local_uri_path("file:///c:/work%20%E9%9B%AA/Main.java", true)
        );
        assert_eq!(
            local_uri_path("file:/work%20雪/Main.java", false),
            local_uri_path("file:///work%20%E9%9B%AA/Main.java", false)
        );
        assert_ne!(
            local_uri_path("file:/C:/work/Main.java", false),
            local_uri_path("file:///c:/work/Main.java", false)
        );
        assert_ne!(
            local_uri_path("file:/C:/work/Main.java", true),
            local_uri_path("file:///C:/work/main.java", true)
        );
        for uri in [
            "file://host/C:/work/Main.java",
            "file://localhost/C:/work/Main.java",
            "file:////C:/work/Main.java",
            "file:C:/work/Main.java",
            "file:///C:/work/Main.java?query",
            "file:///C:/work/Main.java#fragment",
            "file:///C:/work/Main.java%00",
            "file:///C:/work/Main.java%0a",
            "file:///C:/work/Main.java%C2%85",
            "file:///C:/work/Main.java\0",
            "file:///C:/work/Main.java%ZZ",
            "file:///C:/work/Main.java%",
            "file:///C:/work/%FF.java",
            "file:///C:/work%2fMain.java",
            "file:///C:/work%5cMain.java",
            "file:///C:/work\\Main.java",
            "file:///C:/work/../Main.java",
            "file:///C:/work/%2e%2e/Main.java",
            "file:///C:/work/./Main.java",
            "file:///C:/work//Main.java",
            "https:///C:/work/Main.java",
        ] {
            assert!(local_uri_path(uri, true).is_none(), "{uri}");
            assert!(local_uri_path(uri, false).is_none(), "{uri}");
        }
        for uri in [
            "file:///C|/work/Main.java",
            "file:///C:/work/Main.java.",
            "file:///C:/work/Main.java%20",
            "file:///C:/work/NUL.java",
            "file:///C:/work/CoM1.java",
            "file:///C:/work/LPT%C2%B9.java",
            "file:///C:/work/Main.java:stream",
            "file:///Device/HarddiskVolume1/Main.java",
            "file:///%3f%3f/C:/work/Main.java",
        ] {
            assert!(local_uri_path(uri, true).is_none(), "{uri}");
        }
        assert!(local_uri_path(
            &format!(
                "file:///{}",
                "x".repeat(language_results::MAX_DIAGNOSTIC_URI_BYTES)
            ),
            false
        )
        .is_none());
    }
}
