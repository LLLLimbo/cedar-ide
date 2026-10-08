//! Complete, child-local Unicode environment blocks. Never changes host state.
//!
//! Windows requires names sorted in case-insensitive Unicode order, without a
//! locale. CompareStringOrdinal uses the OS uppercase table, avoiding Rust
//! Unicode folding/expansion and lossy conversion of Windows OsStr values.
//! https://learn.microsoft.com/en-us/windows/win32/procthread/changing-environment-variables
//! https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-comparestringordinal
//! https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw

use crate::MAX_ENVIRONMENT_UTF16_UNITS;
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Globalization::{
    CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
};

pub(crate) struct EnvironmentBlock {
    units: Vec<u16>,
}

struct Entry {
    units: Vec<u16>,
    name_len: usize,
}

impl Entry {
    fn name(&self) -> &[u16] {
        &self.units[..self.name_len]
    }
}

impl EnvironmentBlock {
    pub(super) fn new<I, K, V>(environment: I) -> io::Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let mut entries = Vec::new();
        // Reserve the final block NUL before counting any entry. Check each
        // UTF-16 unit before copying it, so even one oversized input is bounded.
        let mut total = 1;
        for (name, value) in environment {
            let mut units = Vec::new();
            for unit in name.as_ref().encode_wide() {
                if unit == 0 || unit == u16::from(b'=') {
                    return Err(invalid("environment names must not contain NUL or '='"));
                }
                push_bounded(&mut units, &mut total, unit)?;
            }
            let name_len = units.len();
            if name_len == 0 {
                return Err(invalid("environment names must not be empty"));
            }
            push_bounded(&mut units, &mut total, u16::from(b'='))?;
            for unit in value.as_ref().encode_wide() {
                if unit == 0 {
                    return Err(invalid("environment values must not contain NUL"));
                }
                push_bounded(&mut units, &mut total, unit)?;
            }
            push_bounded(&mut units, &mut total, 0)?;
            entries.push(Entry { units, name_len });
        }

        let mut comparison_error = None;
        entries.sort_unstable_by(|left, right| {
            match compare_names(left.name(), right.name()) {
                Ok(order) => order,
                Err(error) => {
                    // Valid nonempty bounded slices satisfy the documented API
                    // requirements. Still retain an unexpected OS failure and
                    // reject the block before any process or handle is created.
                    comparison_error.get_or_insert(error);
                    Ordering::Equal
                }
            }
        });
        if let Some(error) = comparison_error {
            return Err(error);
        }
        for pair in entries.windows(2) {
            if compare_names(pair[0].name(), pair[1].name())? == Ordering::Equal {
                return Err(invalid("environment names must be unique ignoring case"));
            }
        }

        let mut units = Vec::with_capacity(total.max(2));
        for entry in entries {
            units.extend(entry.units);
        }
        if units.is_empty() {
            // A non-null empty block is distinct from inherited environment.
            units.push(0);
        }
        units.push(0);
        Ok(Self { units })
    }

    pub(super) fn as_ptr(&self) -> *const u16 {
        self.units.as_ptr()
    }
}

fn push_bounded(units: &mut Vec<u16>, total: &mut usize, unit: u16) -> io::Result<()> {
    if *total >= MAX_ENVIRONMENT_UTF16_UNITS {
        return Err(invalid(
            "explicit environment exceeds the UTF-16 block limit",
        ));
    }
    units.push(unit);
    *total += 1;
    Ok(())
}

fn compare_names(left: &[u16], right: &[u16]) -> io::Result<Ordering> {
    // SAFETY: both slices are live, nonempty, and shorter than the block bound
    // (1 Mi UTF-16 units), hence their lengths fit i32. Explicit lengths avoid
    // requiring a terminator. TRUE is exactly 1 as required by this function.
    // No UTF-8 conversion or normalization changes the original code units.
    let result = unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    };
    match result {
        CSTR_LESS_THAN => Ok(Ordering::Less),
        CSTR_EQUAL => Ok(Ordering::Equal),
        CSTR_GREATER_THAN => Ok(Ordering::Greater),
        _ => Err(io::Error::last_os_error()),
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    fn encoded(pairs: impl IntoIterator<Item = (&'static str, &'static str)>) -> Vec<u16> {
        EnvironmentBlock::new(pairs).unwrap().units
    }

    #[test]
    fn empty_environment_and_empty_values_have_double_terminators() {
        assert_eq!(encoded([]), [0, 0]);
        assert_eq!(encoded([("A", "")]), [65, 61, 0, 0]);
    }

    #[test]
    fn malformed_names_and_nul_values_are_rejected_without_echoing_data() {
        for pair in [
            ("", "value"),
            ("bad\0name", "value"),
            ("bad=name", "value"),
            ("=C:", "C:\\private"),
            ("name", "private\0value"),
        ] {
            let error = EnvironmentBlock::new([pair]).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(!error.to_string().contains("private"));
        }
    }

    #[test]
    fn case_insensitive_duplicates_use_windows_unicode_comparison() {
        for (left, right) in [("Path", "PATH"), ("ä", "Ä"), ("σ", "Σ")] {
            let error = EnvironmentBlock::new([(left, "one"), (right, "two")])
                .err()
                .unwrap();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        // Ordinal comparison does not perform canonical normalization.
        assert!(EnvironmentBlock::new([("é", "one"), ("e\u{301}", "two")]).is_ok());
    }

    #[test]
    fn names_are_sorted_without_a_locale_and_values_stay_literal() {
        let actual = encoded([
            ("雪", "CJK name"),
            ("z", "last"),
            ("a_", "percent% !^ & | < > $(literal)\n=\"quoted\""),
            ("é", "accent name"),
            ("aa", "雪🚀"),
            ("Ä", "uppercase name"),
            ("A", "first"),
        ]);
        let expected: Vec<u16> = concat!(
            "A=first\0",
            "aa=雪🚀\0",
            "a_=percent% !^ & | < > $(literal)\n=\"quoted\"\0",
            "z=last\0",
            "Ä=uppercase name\0",
            "é=accent name\0",
            "雪=CJK name\0\0"
        )
        .encode_utf16()
        .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn borrowed_and_owned_os_strings_preserve_unpaired_surrogates() {
        let name = OsString::from_wide(&[0xd800, 65]);
        let value = OsString::from_wide(&[0xdc00, 0xd83d, 0xde80]);
        let scalar_name = OsString::from("A");
        let empty = OsString::new();
        let expected = [65, 61, 0, 0xd800, 65, 61, 0xdc00, 0xd83d, 0xde80, 0, 0];
        let borrowed = EnvironmentBlock::new([(&name, &value), (&scalar_name, &empty)]).unwrap();
        let owned = EnvironmentBlock::new([(name, value), (scalar_name, empty)]).unwrap();
        assert_eq!(borrowed.units, expected);
        assert_eq!(owned.units, expected);
    }

    #[test]
    fn block_limit_includes_separators_and_both_terminators() {
        let value = OsString::from_wide(&vec![65; MAX_ENVIRONMENT_UTF16_UNITS - 4]);
        let block = EnvironmentBlock::new([(OsStr::new("K"), value.as_os_str())]).unwrap();
        assert_eq!(block.units.len(), MAX_ENVIRONMENT_UTF16_UNITS);
        assert_eq!(&block.units[block.units.len() - 2..], &[0, 0]);
        // The budget is shared across all entries, not reset for each one.
        assert!(EnvironmentBlock::new([("K", &value), ("Z", &OsString::new())]).is_err());
        let mut too_large = value;
        too_large.push("A");
        assert_eq!(
            EnvironmentBlock::new([("K", &too_large)])
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn supplementary_characters_count_as_two_utf16_units() {
        let value = "🚀".repeat((MAX_ENVIRONMENT_UTF16_UNITS - 4) / 2);
        let block = EnvironmentBlock::new([("K", &value)]).unwrap();
        assert_eq!(block.units.len(), MAX_ENVIRONMENT_UTF16_UNITS);
        assert!(EnvironmentBlock::new([("KK", &value)]).is_err());
    }
}
