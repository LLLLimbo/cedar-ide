//! Literal Windows command-line encoding, independent of process creation.
//!
//! The wire protocol supplies UTF-8 strings. Windows C and Rust programs read
//! UTF-16 arguments with a special, non-escaping rule for `argv[0]` and the CRT
//! quote/backslash rules for later arguments. This module does not interpret
//! shell syntax, expand variables, resolve executables, or touch the environment.
//!
//! References used to verify the rules (implementation and oracle are original):
//! - <https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments>
//! - <https://raw.githubusercontent.com/rust-lang/rust/1.99.0/library/std/src/sys/args/windows.rs>
//! - <https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw>

use std::io;

/// `CreateProcessW`'s limit, including the terminating UTF-16 NUL.
pub const MAX_COMMAND_LINE_UNITS: usize = 32_767;

const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;

/// Encode a mutable, NUL-terminated Windows command line without a shell.
///
/// `program` is encoded as `argv[0]`, whose quotes delimit text but whose
/// backslashes never escape anything. Arguments use the Microsoft C / Rust
/// argument rules. Empty arguments, Unicode and literal shell punctuation are
/// preserved. Programs using their own command-line parser may have different
/// rules; this is not a shell-command encoder.
///
/// Rejects an empty program, a quote or NUL in the program, a NUL in any argument,
/// or a complete result exceeding [`MAX_COMMAND_LINE_UNITS`]. The first pass
/// checks the encoded size without allocating; the second allocates only the
/// validated number of UTF-16 units. No input is truncated or normalized.
///
/// This function does not authorize or resolve `program`. A process-creation
/// caller must separately validate the executable and pass its absolute path as
/// `CreateProcessW`'s non-null `lpApplicationName`.
pub fn encode_command_line(program: &str, args: &[String]) -> io::Result<Vec<u16>> {
    validate_program_text(program)?;

    let mut length = 0usize;
    emit_command_line(program, args, &mut |_| {
        if length == MAX_COMMAND_LINE_UNITS {
            return Err(invalid_input(
                "Windows command line exceeds 32767 UTF-16 units",
            ));
        }
        length += 1;
        Ok(())
    })?;

    let mut output = Vec::with_capacity(length);
    emit_command_line(program, args, &mut |unit| {
        output.push(unit);
        Ok(())
    })?;
    debug_assert_eq!(output.len(), length);
    Ok(output)
}

/// Text-only validation for the native executable policy.
///
/// It deliberately does not determine absoluteness, access the filesystem,
/// expand a missing suffix or change the spelling. The Windows owner performs
/// the path and ordinary-file checks before attempting `CreateProcessW`.
#[cfg(any(windows, test))]
pub(crate) fn validate_executable_text(program: &str) -> io::Result<()> {
    validate_program_text(program)?;
    let filename = program.rsplit(['\\', '/']).next().unwrap_or_default();
    if filename.len() <= 4
        || !filename.as_bytes()[filename.len() - 4..].eq_ignore_ascii_case(b".exe")
    {
        return Err(invalid_input(
            "Windows executable must have an explicit .exe extension",
        ));
    }
    Ok(())
}

fn validate_program_text(program: &str) -> io::Result<()> {
    if program.is_empty()
        || program
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, 0 | b'"'))
    {
        return Err(invalid_input(
            "Windows program must be nonempty and contain no NUL or quote",
        ));
    }
    Ok(())
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

// Both passes use precisely the same emission path, so escaped-size accounting
// cannot drift from encoding. The counting sink stops at the first excess unit.
fn emit_command_line(
    program: &str,
    args: &[String],
    emit: &mut impl FnMut(u16) -> io::Result<()>,
) -> io::Result<()> {
    // Unlike ordinary arguments, even a final backslash stays literal here.
    emit(QUOTE)?;
    for unit in program.encode_utf16() {
        emit(unit)?;
    }
    emit(QUOTE)?;

    for arg in args {
        emit(b' ' as u16)?;
        let quoted = arg.is_empty()
            || arg
                .as_bytes()
                .iter()
                .any(|byte| matches!(byte, b' ' | b'\t'));
        if quoted {
            emit(QUOTE)?;
        }

        // Emit backslashes immediately. Only a subsequent quote requires one
        // additional backslash per preceding backslash, plus its own escape.
        let mut backslashes = 0usize;
        for unit in arg.encode_utf16() {
            match unit {
                0 => return Err(invalid_input("Windows arguments must not contain NUL")),
                BACKSLASH => backslashes += 1,
                QUOTE => {
                    for _ in 0..=backslashes {
                        emit(BACKSLASH)?;
                    }
                    backslashes = 0;
                }
                _ => backslashes = 0,
            }
            emit(unit)?;
        }
        if quoted {
            for _ in 0..backslashes {
                emit(BACKSLASH)?;
            }
            emit(QUOTE)?;
        }
    }
    emit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    // Independent parser oracle: consume characters one at a time and, on each
    // quote, correct the already-emitted raw backslash run. This does not call
    // the encoder, share its emission/counting machinery, or use shell32's
    // CommandLineToArgvW (which differs from the modern C/Rust parser).
    fn parse_crt_command_line(input: &[u16]) -> Vec<String> {
        assert_eq!(input.last(), Some(&0));
        let input = &input[..input.len() - 1];
        assert!(!input.contains(&0));
        let mut result = Vec::new();
        let mut cursor = 0;
        let mut quoted = false;
        let mut program = Vec::new();
        while cursor < input.len() {
            let unit = input[cursor];
            cursor += 1;
            if unit == QUOTE {
                quoted = !quoted;
            } else if !quoted && matches!(unit, 9 | 32) {
                break;
            } else {
                program.push(unit);
            }
        }
        result.push(String::from_utf16(&program).unwrap());

        while cursor < input.len() {
            while cursor < input.len() && matches!(input[cursor], 9 | 32) {
                cursor += 1;
            }
            if cursor == input.len() {
                break;
            }
            let mut arg = Vec::new();
            let mut quoted = false;
            while cursor < input.len() {
                let unit = input[cursor];
                if !quoted && matches!(unit, 9 | 32) {
                    break;
                }
                if unit == QUOTE {
                    let preceding_slashes = input[..cursor]
                        .iter()
                        .rev()
                        .take_while(|&&previous| previous == BACKSLASH)
                        .count();
                    arg.truncate(arg.len() - preceding_slashes.div_ceil(2));
                    if preceding_slashes % 2 == 1 {
                        arg.push(QUOTE);
                    } else if quoted && input.get(cursor + 1) == Some(&QUOTE) {
                        arg.push(QUOTE);
                        cursor += 1;
                    } else {
                        quoted = !quoted;
                    }
                } else {
                    arg.push(unit);
                }
                cursor += 1;
            }
            result.push(String::from_utf16(&arg).unwrap());
        }
        result
    }

    fn assert_round_trip(program: &str, args: &[String]) -> Vec<u16> {
        let line = encode_command_line(program, args).unwrap();
        assert!(line.len() <= MAX_COMMAND_LINE_UNITS);
        let mut expected = vec![program.to_owned()];
        expected.extend_from_slice(args);
        assert_eq!(parse_crt_command_line(&line), expected);
        line
    }

    #[test]
    fn independent_oracle_matches_documented_crt_examples() {
        let cases: &[(&str, &[&str])] = &[
            (r#""a b c" d e"#, &["a b c", "d", "e"]),
            (r#""ab\"c" "\\" d"#, &["ab\"c", "\\", "d"]),
            (r#"a\\\b d"e f"g h"#, &[r"a\\\b", "de fg", "h"]),
            (r#"a\\\"b c d"#, &[r#"a\"b"#, "c", "d"]),
            (r#"a\\\\"b c" d e"#, &[r"a\\b c", "d", "e"]),
            (r#"a"b"" c d"#, &["ab\" c d"]),
            (r#""" "" x """#, &["", "", "x", ""]),
        ];
        for (command_tail, expected) in cases {
            let line: Vec<u16> = format!("p {command_tail}\0").encode_utf16().collect();
            let parsed = parse_crt_command_line(&line);
            assert_eq!(&parsed[1..], *expected, "{command_tail}");
        }
    }

    #[test]
    fn empty_arguments_and_literal_shell_characters_round_trip() {
        assert_round_trip(
            "C:\\Program Files\\tool.exe",
            &strings(&[
                "",
                " ",
                "\t",
                "",
                "%PATH%",
                "!name!",
                "$HOME",
                "${x}",
                "$(cmd)",
                "&|<>^;",
                "'single quotes'",
                "a\nb\rc",
                "\u{000b}\u{000c}",
                "",
                "",
            ]),
        );
    }

    #[test]
    fn unicode_is_preserved_without_normalization() {
        let program = "C:\\工具 😀\\程序.exe";
        let args = strings(&[
            "中文",
            "😀🦀",
            "e\u{0301}",
            "é",
            "a\u{00a0}b",
            "\u{feff}",
            "\u{10ffff}",
        ]);
        assert_round_trip(program, &args);
    }

    #[test]
    fn argv_zero_uses_its_own_backslash_rules() {
        for program in [
            r"C:\Program Files\tool.exe",
            "C:\\program with space\\",
            "C:\\a\tb\\",
            r"\\?\C:\工具\tool.exe",
            "\\",
        ] {
            let line = assert_round_trip(program, &strings(&["", "after"]));
            let expected_prefix = format!("\"{program}\" ").encode_utf16().collect::<Vec<_>>();
            assert!(line.starts_with(&expected_prefix));
        }
    }

    #[test]
    fn every_short_quote_backslash_whitespace_and_unicode_string_round_trips() {
        const ALPHABET: &[char] = &['a', ' ', '\t', '\\', '"', '中', '😀'];
        for length in 0..=5u32 {
            for mut combination in 0..ALPHABET.len().pow(length) {
                let mut arg = String::new();
                for _ in 0..length {
                    arg.push(ALPHABET[combination % ALPHABET.len()]);
                    combination /= ALPHABET.len();
                }
                assert_round_trip(
                    "C:\\工具 dir\\p.exe",
                    &["before".into(), arg, "".into(), "after".into()],
                );
            }
        }
    }

    #[test]
    fn long_backslash_runs_before_quotes_and_at_the_end_round_trip() {
        for length in [0, 1, 2, 3, 4, 31, 128, 1024] {
            let slashes = "\\".repeat(length);
            let args = vec![
                slashes.clone(),
                format!(" {slashes}"),
                format!("{slashes}\""),
                format!(" {slashes}\"{slashes}"),
            ];
            assert_round_trip("p", &args);
        }
    }

    #[test]
    fn invalid_program_or_argument_text_is_rejected() {
        for program in ["", "\0", "p\0tail", "p\"tail", "\"p\""] {
            assert_eq!(
                encode_command_line(program, &[]).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        for arg in ["\0", "before\0after", "😀\0"] {
            assert_eq!(
                encode_command_line("p", &strings(&["valid", arg]))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn executable_text_requires_an_exact_nonempty_exe_filename() {
        for program in [
            r"C:\tool.exe",
            r"C:\Program Files\工具.EXE",
            r"\\?\C:\x.ExE",
        ] {
            validate_executable_text(program).unwrap();
        }
        for program in [
            "",
            "tool",
            "tool.bat",
            "tool.cmd",
            "tool.com",
            "tool.exe ",
            "tool.exe.",
            "tool.exe\0",
            "\"tool.exe\"",
            r"C:\.exe",
            "C:/dir.exe/",
            r"C:\dir.exe\",
            "ééé",
            "中中",
            "abc😀",
            "😀",
        ] {
            assert_eq!(
                validate_executable_text(program).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{program:?}"
            );
        }
    }

    #[test]
    fn exact_limit_includes_the_program_quotes_and_terminator() {
        let program = "p".repeat(MAX_COMMAND_LINE_UNITS - 3);
        assert_eq!(
            assert_round_trip(&program, &[]).len(),
            MAX_COMMAND_LINE_UNITS
        );
        assert_eq!(
            encode_command_line(&(program + "p"), &[])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn exact_limit_includes_argument_separator_and_escaping() {
        // Quoted argv[0] (3), one separator (1), terminator (1).
        let arg = "a".repeat(MAX_COMMAND_LINE_UNITS - 5);
        assert_eq!(
            assert_round_trip("p", std::slice::from_ref(&arg)).len(),
            MAX_COMMAND_LINE_UNITS
        );
        assert!(encode_command_line("p", &[arg + "a"]).is_err());

        // Each literal quote becomes backslash + quote. No wrapping is needed.
        let arg = "\"".repeat((MAX_COMMAND_LINE_UNITS - 5) / 2);
        assert_eq!(
            assert_round_trip("p", std::slice::from_ref(&arg)).len(),
            MAX_COMMAND_LINE_UNITS
        );
        assert!(encode_command_line("p", &[arg + "\""]).is_err());

        // A leading space adds enclosing quotes; trailing backslashes double.
        let arg = format!(" a{}", "\\".repeat((MAX_COMMAND_LINE_UNITS - 9) / 2));
        assert_eq!(
            assert_round_trip("p", std::slice::from_ref(&arg)).len(),
            MAX_COMMAND_LINE_UNITS
        );
        assert!(encode_command_line("p", &[arg + "\\"]).is_err());
    }

    #[test]
    fn limit_counts_utf16_units_instead_of_utf8_bytes_or_scalars() {
        let arg = "😀".repeat((MAX_COMMAND_LINE_UNITS - 5) / 2);
        assert_eq!(
            assert_round_trip("p", std::slice::from_ref(&arg)).len(),
            MAX_COMMAND_LINE_UNITS
        );
        assert!(encode_command_line("p", &[arg + "😀"]).is_err());
        let bmp_arg = "中".repeat(MAX_COMMAND_LINE_UNITS - 5);
        assert_eq!(
            assert_round_trip("p", &[bmp_arg]).len(),
            MAX_COMMAND_LINE_UNITS
        );
    }

    #[test]
    fn many_empty_arguments_count_their_separators_and_quotes() {
        let mut args = vec![String::new(); (MAX_COMMAND_LINE_UNITS - 4) / 3];
        assert_eq!(assert_round_trip("p", &args).len(), MAX_COMMAND_LINE_UNITS);
        args.push(String::new());
        assert!(encode_command_line("p", &args).is_err());
    }

    #[test]
    fn huge_inputs_fail_during_the_no_allocation_counting_pass() {
        assert!(encode_command_line(&"p".repeat(1024 * 1024), &[]).is_err());
        assert!(encode_command_line("p", &["\\".repeat(1024 * 1024)]).is_err());
        assert!(encode_command_line("p", &["\"".repeat(1024 * 1024)]).is_err());
        assert!(encode_command_line("p", &vec![String::new(); 100_000]).is_err());
    }
}
