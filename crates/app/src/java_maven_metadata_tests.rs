//! Test-only classifier for one narrow kind of generated m2e bookkeeping.
//! Maven 3.9.16's public lifecycle bindings name these six plugin coordinates.
//! m2e MavenImpl (638f58b86637d88ec518adb4a80061cec6b3e07d) records failed
//! artifact resolution using Properties.store(outputStream, null): a date
//! comment and repository-ID|URL|classifier = System.currentTimeMillis().
//! This does not identify files from an earlier run whose names were not kept.
use std::path::Path;

pub(crate) const MAX_METADATA_BYTES: usize = 32 * 1024;
const MAX_COMMENT_BYTES: usize = 256;
// Bit positions are part of the native receipt contract. Never accept another
// version, plugin, filename, artifact, or cache parent through this list.
const PARENTS: [&str; 6] = [
    "org/apache/maven/plugins/maven-clean-plugin/3.2.0",
    "org/apache/maven/plugins/maven-site-plugin/3.12.1",
    "org/apache/maven/plugins/maven-surefire-plugin/3.5.4",
    "org/apache/maven/plugins/maven-jar-plugin/3.5.0",
    "org/apache/maven/plugins/maven-install-plugin/3.1.4",
    "org/apache/maven/plugins/maven-deploy-plugin/3.1.4",
];
const FILENAME: &str = "m2e-lastUpdated.properties";

fn stored_key(mirror_uri: &str) -> Option<String> {
    if mirror_uri.len() > MAX_METADATA_BYTES
        || !mirror_uri.is_ascii()
        || mirror_uri.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    let uri = url::Url::parse(mirror_uri).ok()?;
    if uri.scheme() != "file"
        || uri.host_str().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
        || uri.as_str() != mirror_uri
        || !uri.path().ends_with("/empty-mirror/")
    {
        return None;
    }
    let key = format!("cedar-owned-file-only|{mirror_uri}|null");
    let mut escaped = String::with_capacity(key.len() + 8);
    // The derived URI is ASCII. Match the actual JDK Properties.store key
    // encoding, rather than accepting alternative escape forms or continuations.
    for character in key.chars() {
        if matches!(character, '\\' | ' ' | ':' | '=' | '#' | '!') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('=');
    Some(escaped)
}

/// Return the one coordinate bit only for the exact writer output expected for
/// the caller's independently verified owned mirror. The Windows caller must
/// also verify that the file is ordinary, bounded, and not a reparse point.
pub(crate) fn lifecycle_metadata_bit(
    relative: &Path,
    contents: &[u8],
    owned_mirror_uri: &str,
) -> Option<u8> {
    let relative = relative.to_str()?.replace('\\', "/");
    let index = PARENTS
        .iter()
        .position(|parent| relative == format!("{parent}/{FILENAME}"))?;
    if contents.is_empty() || contents.len() > MAX_METADATA_BYTES {
        return None;
    }
    let encoded = std::str::from_utf8(contents).ok()?;
    let newline = if encoded.ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let body = encoded.strip_suffix(newline)?;
    let (comment, property) = body.split_once(newline)?;
    if !(2..=MAX_COMMENT_BYTES).contains(&comment.len())
        || !comment.starts_with('#')
        || !comment
            .bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        || property.contains(['\r', '\n'])
    {
        return None;
    }
    let key = stored_key(owned_mirror_uri)?;
    let timestamp = property.strip_prefix(&key)?;
    if !(1..=19).contains(&timestamp.len())
        || timestamp.starts_with('0')
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    // System.currentTimeMillis() is a Java signed long. The fixture runs after
    // the epoch, so reject zero, signs, leading zeroes, fractions and overflow.
    timestamp.parse::<i64>().ok().filter(|value| *value > 0)?;
    Some(1 << index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIRROR: &str = "file:///C:/owned%20data/cedar-maven-Ab12cd/empty-mirror/";
    const LINUX_MIRROR: &str = "file:///tmp/owned/cedar-maven-Ab12cd/empty-mirror/";

    fn path(index: usize) -> String {
        format!("{}/{FILENAME}", PARENTS[index])
    }
    fn properties(mirror: &str, timestamp: &str, newline: &str) -> Vec<u8> {
        format!(
            "#Fri Oct 09 03:00:00 UTC 2026{newline}{}{timestamp}{newline}",
            stored_key(mirror).unwrap()
        )
        .into_bytes()
    }
    fn classify(path: &str, contents: &[u8]) -> Option<u8> {
        lifecycle_metadata_bit(Path::new(path), contents, MIRROR)
    }

    #[test]
    fn exact_six_coordinates_produce_distinct_receipt_bits() {
        for mirror in [MIRROR, LINUX_MIRROR] {
            for newline in ["\n", "\r\n"] {
                let contents = properties(mirror, "1791509159303", newline);
                let mut mask = 0u8;
                for index in 0..6 {
                    let bit =
                        lifecycle_metadata_bit(Path::new(&path(index)), &contents, mirror).unwrap();
                    assert_eq!(bit, 1 << index);
                    mask |= bit;
                }
                assert_eq!(mask, 63);
                assert_eq!(mask.count_ones(), 6);
            }
        }
    }

    #[test]
    fn coordinate_version_parent_filename_and_artifacts_are_exact() {
        let contents = properties(MIRROR, "1791509159303", "\n");
        for wrong in [
            "org/apache/maven/plugins/maven-clean-plugin/3.2.1/m2e-lastUpdated.properties",
            "org/apache/maven/plugins/maven-compiler-plugin/3.15.0/m2e-lastUpdated.properties",
            "org/apache/maven/other/maven-clean-plugin/3.2.0/m2e-lastUpdated.properties",
            "org/apache/maven/plugins/maven-clean-plugin/3.2.0/child/m2e-lastUpdated.properties",
            "org/apache/maven/plugins/maven-clean-plugin/3.2.0/_remote.repositories",
            "org/apache/maven/plugins/maven-clean-plugin/3.2.0/resolver-status.properties",
            "org/apache/maven/plugins/maven-clean-plugin/3.2.0/maven-clean-plugin-3.2.0.jar.lastUpdated",
            "org/apache/maven/plugins/maven-clean-plugin/3.2.0/maven-clean-plugin-3.2.0.jar",
            "org/apache/maven/plugins/maven-clean-plugin/3.2.0/maven-clean-plugin-3.2.0.pom",
        ] {
            assert_eq!(classify(wrong, &contents), None);
        }
        for prefix in ["/", "../", "foreign/", "C:/", "./"] {
            assert_eq!(classify(&format!("{prefix}{}", path(0)), &contents), None);
        }
    }

    #[test]
    fn exact_owned_file_mirror_and_writer_escaping_are_required() {
        let contents = properties(MIRROR, "1", "\n");
        assert_eq!(stored_key(MIRROR).unwrap(), "cedar-owned-file-only|file\\:///C\\:/owned%20data/cedar-maven-Ab12cd/empty-mirror/|null=");
        let punctuation_mirror = "file:///C:/owned!part=one/cedar-maven-Ab12cd/empty-mirror/";
        assert!(stored_key(punctuation_mirror)
            .unwrap()
            .contains("owned\\!part\\=one"));
        assert_eq!(
            lifecycle_metadata_bit(
                Path::new(&path(0)),
                &properties(punctuation_mirror, "1", "\n"),
                punctuation_mirror
            ),
            Some(1)
        );
        for wrong_mirror in [
            "file:///C:/foreign/cedar-maven-Ab12cd/empty-mirror/",
            "file:///C:/owned%20data/cedar-maven-Other1/empty-mirror/",
            "https://example.com/empty-mirror/",
            "file://server/share/empty-mirror/",
            "file:///C:/owned/empty-mirror/?x=1",
            "file:///C:/owned/empty-mirror/#fragment",
            "file:///C:/owned/empty-mirror",
            "file:///C:/owned/empty-mirror/\n",
        ] {
            assert_eq!(
                lifecycle_metadata_bit(Path::new(&path(0)), &contents, wrong_mirror),
                None
            );
        }
        let exact = String::from_utf8(contents).unwrap();
        for wrong in [
            exact.replace("cedar-owned-file-only|", "central|"),
            exact.replace("|null=", "|sources="),
            exact.replace("|null=", "|javadoc="),
            exact.replace("file\\:", "file:"),
            exact.replace("C\\:", "C:"),
            exact.replace("cedar-owned", "\\u0063edar-owned"),
            exact.replace("|null=", "|null\\\n="),
            exact.replace("Ab12cd", "Wrong1"),
        ] {
            assert_eq!(classify(&path(0), wrong.as_bytes()), None);
        }
    }

    #[test]
    fn timestamp_values_and_property_cardinality_are_bounded() {
        for value in ["1", "1791509159303", "9223372036854775807"] {
            assert_eq!(
                classify(&path(0), &properties(MIRROR, value, "\n")),
                Some(1)
            );
        }
        for value in [
            "",
            "0",
            "01",
            "+1",
            "-1",
            " 1",
            "1 ",
            "1.0",
            "1e3",
            "NaN",
            "9223372036854775808",
            "18446744073709551616",
            "1\nother=2",
        ] {
            assert_eq!(classify(&path(0), &properties(MIRROR, value, "\n")), None);
        }
        let exact = String::from_utf8(properties(MIRROR, "1", "\n")).unwrap();
        let property = exact.lines().nth(1).unwrap();
        for wrong in [
            String::new(),
            exact.trim_end().into(),
            format!("{exact}\n"),
            format!("{exact}{property}\n"),
            format!("{exact}{}2\n", stored_key(MIRROR).unwrap()),
            format!("{exact}another=2\n"),
            format!("#another\n{exact}"),
            format!("{}\n{property}\n", "#".repeat(257)),
            exact.replacen('#', "!", 1),
            exact.replace("UTC", "U\tTC"),
            exact.replace("2026\n", "2026\r\n"),
            format!("{exact}{}", "x".repeat(MAX_METADATA_BYTES)),
        ] {
            assert_eq!(classify(&path(0), wrong.as_bytes()), None);
        }
        assert_eq!(classify(&path(0), b"#date\n\xff\n"), None);
    }
}
