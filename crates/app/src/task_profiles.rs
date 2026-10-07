//! Strict, bounded task-profile data. Parsing and encoding never run a command.

use cedar_tasks::{MAX_ARGUMENTS, MAX_ARGUMENT_BYTES, MAX_PROGRAM_BYTES, MAX_TIMEOUT};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Write};

pub const MAX_TASK_FILE_BYTES: usize = 256 * 1024;
pub const MAX_PROFILES: usize = 32;
pub const MAX_PROFILE_NAME_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskFile {
    pub version: u32,
    #[serde(deserialize_with = "deserialize_profile_objects")]
    pub profiles: Vec<TaskProfile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProfile {
    pub name: String,
    pub program: String,
    #[serde(deserialize_with = "deserialize_arguments")]
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

/// Read version 1 without defaults, key coercions, shell parsing, or trimming.
pub fn parse_task_file(text: &str) -> Result<TaskFile, String> {
    if text.len() > MAX_TASK_FILE_BYTES {
        return Err(file_size_error());
    }
    // Serde's derived struct visitor also accepts positional arrays. A task
    // file must use named JSON fields, including each profile object below.
    if !text.trim_start().starts_with('{') {
        return Err("Task file must be a JSON object".into());
    }
    let file: TaskFile =
        serde_json::from_str(text).map_err(|error| format!("Invalid task file JSON: {error}"))?;
    validate_file(&file)?;
    // Input and output budgets are independent. A compact file near the cap
    // remains readable even if pretty-printing it would exceed the save limit.
    Ok(file)
}

/// Check one profile without modifying its executable or literal arguments.
pub fn validate_profile(profile: &TaskProfile) -> Result<(), String> {
    if profile.name.is_empty()
        || profile.name.trim() != profile.name
        || profile.name.len() > MAX_PROFILE_NAME_BYTES
        || profile.name.chars().any(char::is_control)
    {
        return Err(format!(
            "Profile name must be nonempty, trimmed, control-free, and at most {MAX_PROFILE_NAME_BYTES} UTF-8 bytes"
        ));
    }
    if profile.program.is_empty()
        || profile.program.len() > MAX_PROGRAM_BYTES
        || profile.program.contains('\0')
    {
        return Err(format!(
            "Program must be nonempty, contain no NUL, and be at most {MAX_PROGRAM_BYTES} UTF-8 bytes"
        ));
    }
    if profile.args.len() > MAX_ARGUMENTS {
        return Err(format!(
            "A profile supports at most {MAX_ARGUMENTS} arguments"
        ));
    }
    if profile.args.iter().any(|argument| argument.contains('\0')) {
        return Err("Arguments must contain no NUL".into());
    }
    if profile
        .args
        .iter()
        .try_fold(0usize, |sum, argument| sum.checked_add(argument.len()))
        .is_none_or(|bytes| bytes > MAX_ARGUMENT_BYTES)
    {
        return Err(format!(
            "Arguments must total at most {MAX_ARGUMENT_BYTES} UTF-8 bytes"
        ));
    }
    if !(1..=MAX_TIMEOUT.as_secs()).contains(&profile.timeout_secs) {
        return Err(format!(
            "Timeout must be between 1 and {} seconds",
            MAX_TIMEOUT.as_secs()
        ));
    }
    Ok(())
}

/// Produce indented version-1 JSON with one trailing newline, within the byte cap.
pub fn encode_task_file(file: &TaskFile) -> Result<String, String> {
    validate_file(file)?;
    let bytes = serialize_bounded(file)?;
    String::from_utf8(bytes).map_err(|error| format!("Task file encoding failed: {error}"))
}

fn validate_file(file: &TaskFile) -> Result<(), String> {
    if file.version != 1 {
        return Err(format!("Unsupported task file version: {}", file.version));
    }
    if file.profiles.len() > MAX_PROFILES {
        return Err(format!(
            "A task file supports at most {MAX_PROFILES} profiles"
        ));
    }
    let mut names = HashSet::with_capacity(file.profiles.len());
    for (index, profile) in file.profiles.iter().enumerate() {
        validate_profile(profile).map_err(|error| format!("Profile {}: {error}", index + 1))?;
        if !names.insert(profile.name.as_str()) {
            return Err(format!("Profile {} has a duplicate name", index + 1));
        }
    }
    Ok(())
}

fn file_size_error() -> String {
    format!("Task file must be at most {MAX_TASK_FILE_BYTES} encoded UTF-8 bytes")
}

#[derive(Default)]
struct BoundedWriter {
    bytes: Vec<u8>,
    exceeded: bool,
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_TASK_FILE_BYTES.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(std::io::Error::other(file_size_error()));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn serialize_bounded(file: &TaskFile) -> Result<Vec<u8>, String> {
    let mut writer = BoundedWriter::default();
    serde_json::to_writer_pretty(&mut writer, file).map_err(|error| {
        if writer.exceeded {
            file_size_error()
        } else {
            format!("Task file encoding failed: {error}")
        }
    })?;
    // Include the final newline in the same bounded writer, including at the
    // exact boundary where JSON alone fits but its line ending would not.
    writer.write_all(b"\n").map_err(|_| file_size_error())?;
    Ok(writer.bytes)
}

fn deserialize_profile_objects<'de, D>(deserializer: D) -> Result<Vec<TaskProfile>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ProfileObject(TaskProfile);

    impl<'de> Deserialize<'de> for ProfileObject {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            struct ObjectVisitor;

            impl<'de> serde::de::Visitor<'de> for ObjectVisitor {
                type Value = ProfileObject;

                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("a task profile JSON object")
                }

                fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
                where
                    M: serde::de::MapAccess<'de>,
                {
                    TaskProfile::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                        .map(ProfileObject)
                }
            }

            deserializer.deserialize_map(ObjectVisitor)
        }
    }

    struct ProfilesVisitor;

    impl<'de> serde::de::Visitor<'de> for ProfilesVisitor {
        type Value = Vec<TaskProfile>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                formatter,
                "an array of at most {MAX_PROFILES} task profiles"
            )
        }

        fn visit_seq<S>(self, mut sequence: S) -> Result<Self::Value, S::Error>
        where
            S: serde::de::SeqAccess<'de>,
        {
            let mut profiles =
                Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_PROFILES));
            while profiles.len() < MAX_PROFILES {
                let Some(profile) = sequence.next_element::<ProfileObject>()? else {
                    return Ok(profiles);
                };
                profiles.push(profile.0);
            }
            // Detect another entry without allocating its strings or retaining
            // more profiles. Raw input is capped before deserialization begins.
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom(format!(
                    "A task file supports at most {MAX_PROFILES} profiles"
                )));
            }
            Ok(profiles)
        }
    }

    deserializer.deserialize_seq(ProfilesVisitor)
}

fn deserialize_arguments<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ArgumentsVisitor;

    impl<'de> serde::de::Visitor<'de> for ArgumentsVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "an array of at most {MAX_ARGUMENTS} arguments")
        }

        fn visit_seq<S>(self, mut sequence: S) -> Result<Self::Value, S::Error>
        where
            S: serde::de::SeqAccess<'de>,
        {
            let mut args = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_ARGUMENTS));
            let mut total_bytes = 0usize;
            while args.len() < MAX_ARGUMENTS {
                let Some(argument) = sequence.next_element::<String>()? else {
                    return Ok(args);
                };
                if argument.len() > MAX_ARGUMENT_BYTES - total_bytes {
                    return Err(serde::de::Error::custom(format!(
                        "Arguments must total at most {MAX_ARGUMENT_BYTES} UTF-8 bytes"
                    )));
                }
                total_bytes += argument.len();
                args.push(argument);
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom(format!(
                    "A profile supports at most {MAX_ARGUMENTS} arguments"
                )));
            }
            Ok(args)
        }
    }

    deserializer.deserialize_seq(ArgumentsVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"{"version":1,"profiles":[{"name":"Build","program":"cargo","args":["build"],"timeout_secs":300}]}"#;

    fn profile() -> TaskProfile {
        TaskProfile {
            name: "Build".into(),
            program: "cargo".into(),
            args: vec!["build".into()],
            timeout_secs: 300,
        }
    }

    fn task_file(profiles: Vec<TaskProfile>) -> TaskFile {
        TaskFile {
            version: 1,
            profiles,
        }
    }

    fn assert_invalid_both(file: &TaskFile) {
        assert!(encode_task_file(file).is_err());
        assert!(parse_task_file(&serde_json::to_string(file).unwrap()).is_err());
    }

    #[test]
    fn exact_version_one_example_round_trips() {
        let expected = task_file(vec![profile()]);
        assert_eq!(parse_task_file(EXAMPLE).unwrap(), expected);
        let encoded = encode_task_file(&expected).unwrap();
        assert_eq!(
            encoded,
            format!("{}\n", serde_json::to_string_pretty(&expected).unwrap())
        );
        assert!(encoded.starts_with("{\n  \"version\": 1,\n"));
        assert!(encoded.ends_with("}\n"));
        assert!(!encoded.ends_with("}\n\n"));
        assert_eq!(parse_task_file(&encoded).unwrap(), expected);
    }

    #[test]
    fn empty_profile_list_is_valid() {
        let file = task_file(Vec::new());
        assert_eq!(
            parse_task_file(&encode_task_file(&file).unwrap()).unwrap(),
            file
        );
    }

    #[test]
    fn missing_fields_are_rejected_without_defaults() {
        let original = serde_json::to_value(task_file(vec![profile()])).unwrap();
        for key in ["version", "profiles"] {
            let mut value = original.clone();
            value.as_object_mut().unwrap().remove(key);
            assert!(parse_task_file(&value.to_string()).is_err(), "{key}");
        }
        for key in ["name", "program", "args", "timeout_secs"] {
            let mut value = original.clone();
            value["profiles"][0].as_object_mut().unwrap().remove(key);
            assert!(parse_task_file(&value.to_string()).is_err(), "{key}");
        }
    }

    #[test]
    fn unknown_fields_are_rejected_at_both_levels() {
        for text in [
            EXAMPLE.replacen('{', "{\"extra\":true,", 1),
            EXAMPLE.replace("\"name\":", "\"extra\":true,\"name\":"),
        ] {
            assert!(parse_task_file(&text).is_err());
            assert!(serde_json::from_str::<TaskFile>(&text).is_err());
        }
    }

    #[test]
    fn duplicate_keys_are_rejected_at_both_levels() {
        for (key, value) in [
            ("version", "1"),
            ("profiles", "[]"),
            ("name", "\"Build\""),
            ("program", "\"cargo\""),
            ("args", "[]"),
            ("timeout_secs", "300"),
        ] {
            let text = EXAMPLE.replace(
                &format!("\"{key}\":"),
                &format!("\"{key}\":{value},\"{key}\":"),
            );
            assert!(parse_task_file(&text).is_err(), "{key}");
            assert!(serde_json::from_str::<TaskFile>(&text).is_err(), "{key}");
        }
        // Escaping a key does not give it a distinct identity.
        let escaped = EXAMPLE.replace("\"name\":", "\"na\\u006de\":\"Build\",\"name\":");
        assert!(parse_task_file(&escaped).is_err());
    }

    #[test]
    fn wrong_shapes_types_and_trailing_data_are_rejected() {
        for text in [
            "null",
            "[]",
            "[1,[]]",
            r#"{"version":1,"profiles":[["Build","cargo",[],300]]}"#,
            r#"{"version":1,"profiles":null}"#,
            r#"{"version":"1","profiles":[]}"#,
            r#"{"version":1.0,"profiles":[]}"#,
            r#"{"version":-1,"profiles":[]}"#,
            r#"{"version":4294967296,"profiles":[]}"#,
            r#"{"version":1,"profiles":[]} {}"#,
        ] {
            assert!(parse_task_file(text).is_err(), "{text}");
        }
        for (from, to) in [
            ("\"name\":\"Build\"", "\"name\":null"),
            ("\"program\":\"cargo\"", "\"program\":12"),
            ("[\"build\"]", "[12]"),
            ("[\"build\"]", "\"build\""),
            ("300", "1.0"),
            ("300", "-1"),
            ("300", "18446744073709551616"),
        ] {
            assert!(parse_task_file(&EXAMPLE.replace(from, to)).is_err(), "{to}");
        }
    }

    #[test]
    fn only_version_one_is_supported() {
        for version in [0, 2, u32::MAX] {
            let mut file = task_file(vec![profile()]);
            file.version = version;
            assert_invalid_both(&file);
        }
    }

    #[test]
    fn profile_count_boundary_and_duplicate_names() {
        let mut profiles = (0..MAX_PROFILES)
            .map(|index| TaskProfile {
                name: format!("Build {index}"),
                ..profile()
            })
            .collect::<Vec<_>>();
        let file = task_file(profiles.clone());
        assert_eq!(
            parse_task_file(&encode_task_file(&file).unwrap()).unwrap(),
            file
        );
        profiles.push(profile());
        assert_invalid_both(&task_file(profiles));
        assert_invalid_both(&task_file(vec![profile(), profile()]));
    }

    #[test]
    fn deserializer_does_not_materialize_profiles_beyond_the_count_limit() {
        let profiles = (0..MAX_PROFILES)
            .map(|index| TaskProfile {
                name: format!("Build {index}"),
                ..profile()
            })
            .collect();
        let encoded = serde_json::to_string(&task_file(profiles)).unwrap();
        // The 33rd value is not even a profile. The bounded sequence visitor
        // ignores its representation and reports capacity instead of decoding it.
        let oversized = format!("{},null]}}", encoded.strip_suffix("]}").unwrap());
        let error = parse_task_file(&oversized).unwrap_err();
        assert!(error.contains("at most 32 profiles"), "{error}");
    }

    #[test]
    fn names_are_exact_case_sensitive_strings() {
        let file = task_file(
            ["Build", "build", "Café", "Cafe\u{301}"]
                .into_iter()
                .map(|name| TaskProfile {
                    name: name.into(),
                    ..profile()
                })
                .collect(),
        );
        assert_eq!(
            parse_task_file(&encode_task_file(&file).unwrap()).unwrap(),
            file
        );
    }

    #[test]
    fn names_are_nonempty_trimmed_and_control_free() {
        for name in [
            "",
            " ",
            " Build",
            "Build ",
            "\tBuild",
            "Build\n",
            "Build\0x",
            "B\u{7f}uild",
            "B\u{85}uild",
            "\u{2003}Build",
            "Build\u{a0}",
        ] {
            let mut invalid = profile();
            invalid.name = name.into();
            assert!(validate_profile(&invalid).is_err(), "{name:?}");
            assert_invalid_both(&task_file(vec![invalid]));
        }
        let mut valid = profile();
        valid.name = "Build all 项目".into();
        assert!(validate_profile(&valid).is_ok());
    }

    #[test]
    fn name_limit_counts_utf8_bytes() {
        for name in ["a".repeat(MAX_PROFILE_NAME_BYTES), "é".repeat(64)] {
            let mut valid = profile();
            valid.name = name;
            assert!(validate_profile(&valid).is_ok());
            valid.name.push('a');
            assert!(validate_profile(&valid).is_err());
        }
    }

    #[test]
    fn program_limits_match_task_runner_without_trimming() {
        let mut candidate = profile();
        for program in ["".into(), "x\0y".into(), "a".repeat(MAX_PROGRAM_BYTES + 1)] {
            candidate.program = program;
            assert!(validate_profile(&candidate).is_err());
        }
        for program in [
            " ".into(),
            "\t program \n".into(),
            "p".repeat(MAX_PROGRAM_BYTES),
            "é".repeat(MAX_PROGRAM_BYTES / 2),
        ] {
            candidate.program = program;
            assert!(validate_profile(&candidate).is_ok());
            let file = task_file(vec![candidate.clone()]);
            assert_eq!(
                parse_task_file(&encode_task_file(&file).unwrap()).unwrap(),
                file
            );
        }
        candidate.program.push('x');
        assert!(validate_profile(&candidate).is_err());
    }

    #[test]
    fn argument_count_boundary_includes_empty_arguments() {
        let mut candidate = profile();
        candidate.args.clear();
        assert!(validate_profile(&candidate).is_ok());
        candidate.args = vec![String::new(); MAX_ARGUMENTS];
        assert!(validate_profile(&candidate).is_ok());
        candidate.args.push(String::new());
        assert!(validate_profile(&candidate).is_err());
        assert_invalid_both(&task_file(vec![candidate]));
    }

    #[test]
    fn deserializer_does_not_materialize_arguments_beyond_the_count_limit() {
        let candidate = TaskProfile {
            args: vec![String::new(); MAX_ARGUMENTS],
            ..profile()
        };
        let encoded = serde_json::to_string(&task_file(vec![candidate])).unwrap();
        let oversized = encoded.replace("],\"timeout_secs\"", ",null],\"timeout_secs\"");
        let error = parse_task_file(&oversized).unwrap_err();
        assert!(error.contains("at most 256 arguments"), "{error}");
    }

    #[test]
    fn argument_bytes_are_aggregate_utf8_and_reject_nul() {
        let mut candidate = profile();
        candidate.args = vec!["é".repeat(MAX_ARGUMENT_BYTES / 4); 2];
        assert!(validate_profile(&candidate).is_ok());
        candidate.args[1].push('a');
        assert!(validate_profile(&candidate).is_err());
        assert_invalid_both(&task_file(vec![candidate.clone()]));
        candidate.args = vec!["x\0y".into()];
        assert!(validate_profile(&candidate).is_err());
        assert_invalid_both(&task_file(vec![candidate]));
    }

    #[test]
    fn timeout_boundaries_are_inclusive() {
        let mut candidate = profile();
        for timeout_secs in [1, 300] {
            candidate.timeout_secs = timeout_secs;
            assert!(validate_profile(&candidate).is_ok());
        }
        for timeout_secs in [0, 301, u64::MAX] {
            candidate.timeout_secs = timeout_secs;
            assert!(validate_profile(&candidate).is_err());
            assert_invalid_both(&task_file(vec![candidate.clone()]));
        }
    }

    #[test]
    fn literal_shell_looking_arguments_whitespace_and_unicode_are_preserved() {
        let file = task_file(vec![TaskProfile {
            name: "Literal arguments".into(),
            program: " /path with spaces/程序 ".into(),
            args: [
                "",
                " ",
                "\t\n",
                "--flag=value",
                "a b",
                "\"quoted\"",
                "'quoted'",
                "$HOME",
                "${NAME}",
                "$(touch sentinel)",
                "`whoami`",
                "a; b",
                "a && b",
                "a | b",
                "> file",
                "*.rs",
                "~",
                "C:\\a\\b",
                "日本語 🦀",
                "\u{1}",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            timeout_secs: 1,
        }]);
        assert_eq!(
            parse_task_file(&encode_task_file(&file).unwrap()).unwrap(),
            file
        );
    }

    #[test]
    fn input_byte_limit_includes_json_whitespace() {
        let mut text = EXAMPLE.to_owned();
        text.push_str(&" ".repeat(MAX_TASK_FILE_BYTES - text.len()));
        assert!(parse_task_file(&text).is_ok());
        text.push(' ');
        assert_eq!(parse_task_file(&text).unwrap_err(), file_size_error());
    }

    #[test]
    fn encoded_file_limit_is_inclusive_and_aggregate() {
        let mut file = task_file(
            (0..4)
                .map(|index| TaskProfile {
                    name: format!("Build {index}"),
                    args: vec![String::new()],
                    ..profile()
                })
                .collect(),
        );
        let overhead = encode_task_file(&file).unwrap().len();
        for profile in &mut file.profiles[..3] {
            profile.args[0] = "a".repeat(MAX_ARGUMENT_BYTES);
        }
        file.profiles[3].args[0] = "a".repeat(MAX_ARGUMENT_BYTES - overhead);
        let encoded = encode_task_file(&file).unwrap();
        assert_eq!(encoded.len(), MAX_TASK_FILE_BYTES);
        assert_eq!(parse_task_file(&encoded).unwrap(), file);
        file.profiles[3].args[0].push('a');
        assert_eq!(
            serde_json::to_string_pretty(&file).unwrap().len(),
            MAX_TASK_FILE_BYTES
        );
        assert_eq!(encode_task_file(&file).unwrap_err(), file_size_error());
        // The final newline alone now crosses the output cap. The smaller
        // compact source still fits its independent input budget and is usable.
        let compact = serde_json::to_string(&file).unwrap();
        assert!(compact.len() < MAX_TASK_FILE_BYTES);
        assert_eq!(parse_task_file(&compact).unwrap(), file);
    }

    #[test]
    fn escaped_output_expansion_is_bounded_before_allocating_entire_json() {
        let file = task_file(vec![TaskProfile {
            args: vec!["\u{1}".repeat(MAX_ARGUMENT_BYTES)],
            ..profile()
        }]);
        assert!(validate_profile(&file.profiles[0]).is_ok());
        assert_eq!(encode_task_file(&file).unwrap_err(), file_size_error());
        let file = task_file(vec![TaskProfile {
            args: vec!["\\\"\n".repeat(MAX_ARGUMENT_BYTES / 3)],
            ..profile()
        }]);
        assert_eq!(
            parse_task_file(&encode_task_file(&file).unwrap()).unwrap(),
            file
        );
    }

    #[test]
    fn bounded_writer_never_appends_bytes_past_limit() {
        let mut writer = BoundedWriter::default();
        assert!(writer.write_all(&vec![b'a'; MAX_TASK_FILE_BYTES]).is_ok());
        assert_eq!(writer.bytes.len(), MAX_TASK_FILE_BYTES);
        assert!(writer.write_all(b"x").is_err());
        assert!(writer.exceeded);
        assert_eq!(writer.bytes.len(), MAX_TASK_FILE_BYTES);

        let mut writer = BoundedWriter::default();
        assert!(writer
            .write_all(&vec![b'a'; MAX_TASK_FILE_BYTES + 1])
            .is_err());
        assert!(writer.bytes.is_empty());
    }
}
