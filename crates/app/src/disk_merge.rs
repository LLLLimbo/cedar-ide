//! A bounded, pure merge of two strictly separated line-change envelopes.
//!
//! This is deliberately not a general diff or three-way merge. Each side has
//! one envelope, found from independently maximal exact prefixes and suffixes.
//! The caller owns snapshot checks and applying the complete result atomically.

use std::fmt;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_LINES: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MergeError {
    InputTooLarge,
    TooManyInputLines,
    ContainsNul,
    UnchangedSide,
    AmbiguousAlignment,
    TouchingOrOverlapping,
    ResultTooLarge,
    TooManyResultLines,
    NoCombinedChange,
    AllocationFailed,
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InputTooLarge => "Merge refused: an input exceeds the 1 MiB limit",
            Self::TooManyInputLines => "Merge refused: an input exceeds the 65,536-line limit",
            Self::ContainsNul => "Merge refused: an input contains NUL",
            Self::UnchangedSide => {
                "Merge refused: both the draft and disk must differ from the base"
            }
            Self::AmbiguousAlignment => {
                "Merge refused: repeated lines make the change alignment ambiguous"
            }
            Self::TouchingOrOverlapping => "Merge refused: the changes touch or overlap",
            Self::ResultTooLarge => "Merge refused: the result exceeds the 1 MiB limit",
            Self::TooManyResultLines => "Merge refused: the result exceeds the 65,536-line limit",
            Self::NoCombinedChange => {
                "Merge refused: the result would not preserve a separate change from each side"
            }
            Self::AllocationFailed => "Merge refused: the bounded result could not be allocated",
        })
    }
}

impl std::error::Error for MergeError {}

struct Lines<'a> {
    text: &'a str,
    tokens: Vec<&'a str>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Result<Self, MergeError> {
        if text.len() > MAX_BYTES {
            return Err(MergeError::InputTooLarge);
        }
        if text.as_bytes().contains(&0) {
            return Err(MergeError::ContainsNul);
        }
        // Retain every LF and its preceding CR exactly. Empty text has no
        // tokens, and a final LF does not create a synthetic empty token.
        let count = text.split_inclusive('\n').count();
        if count > MAX_LINES {
            return Err(MergeError::TooManyInputLines);
        }
        let mut tokens = Vec::new();
        tokens
            .try_reserve_exact(count)
            .map_err(|_| MergeError::AllocationFailed)?;
        tokens.extend(text.split_inclusive('\n'));
        Ok(Self { text, tokens })
    }
}

struct Change<'a> {
    /// Byte offsets at exact line-token boundaries in the base.
    start: usize,
    end: usize,
    replacement: &'a str,
}

fn change<'a>(base: &Lines<'_>, side: &Lines<'a>) -> Result<Change<'a>, MergeError> {
    if base.text == side.text {
        return Err(MergeError::UnchangedSide);
    }
    let prefix = base
        .tokens
        .iter()
        .zip(&side.tokens)
        .take_while(|(left, right)| left == right)
        .count();
    // Do not stop at the prefix. Overlapping maximal matches are evidence of
    // ambiguous alignment, not permission to pick one repeated-line location.
    let suffix = base
        .tokens
        .iter()
        .rev()
        .zip(side.tokens.iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    if prefix + suffix > base.tokens.len().min(side.tokens.len()) {
        return Err(MergeError::AmbiguousAlignment);
    }
    // Both sums are bounded by the already validated input byte length.
    let start = base.tokens[..prefix].iter().map(|line| line.len()).sum();
    let suffix_bytes: usize = base.tokens[base.tokens.len() - suffix..]
        .iter()
        .map(|line| line.len())
        .sum();
    Ok(Change {
        start,
        end: base.text.len() - suffix_bytes,
        replacement: &side.text[start..side.text.len() - suffix_bytes],
    })
}

/// Merge only when both sides changed, their independently determined base
/// intervals have a strict gap, and the complete result fits the input limits.
/// No input is mutated and no partial result is returned on refusal.
pub(super) fn merge_separate_changes(
    base: &str,
    local: &str,
    disk: &str,
) -> Result<String, MergeError> {
    let base = Lines::new(base)?;
    let local_lines = Lines::new(local)?;
    let disk_lines = Lines::new(disk)?;
    let local_change = change(&base, &local_lines)?;
    let disk_change = change(&base, &disk_lines)?;
    let (first, second) = if local_change.end < disk_change.start {
        (local_change, disk_change)
    } else if disk_change.end < local_change.start {
        (disk_change, local_change)
    } else {
        // Equality also refuses touching replacements and insertions, including
        // two zero-width changes at the same location or an interval boundary.
        return Err(MergeError::TouchingOrOverlapping);
    };
    let parts = [
        &base.text[..first.start],
        first.replacement,
        &base.text[first.end..second.start],
        second.replacement,
        &base.text[second.end..],
    ];
    let mut bytes = 0usize;
    let mut newlines = 0usize;
    let mut last_byte = None;
    for part in parts {
        bytes = bytes
            .checked_add(part.len())
            .filter(|length| *length <= MAX_BYTES)
            .ok_or(MergeError::ResultTooLarge)?;
        newlines = newlines
            .checked_add(part.bytes().filter(|byte| *byte == b'\n').count())
            .ok_or(MergeError::TooManyResultLines)?;
        if let Some(byte) = part.as_bytes().last() {
            last_byte = Some(*byte);
        }
    }
    // Count the concatenated result before allocating it. Counting each part's
    // split tokens separately would mishandle a seam without a newline.
    let lines = newlines
        .checked_add(usize::from(last_byte.is_some_and(|byte| byte != b'\n')))
        .filter(|count| *count <= MAX_LINES)
        .ok_or(MergeError::TooManyResultLines)?;
    debug_assert!(lines <= MAX_LINES);
    let mut result = String::new();
    result
        .try_reserve_exact(bytes)
        .map_err(|_| MergeError::AllocationFailed)?;
    for part in parts {
        result.push_str(part);
    }
    if result == local || result == disk {
        return Err(MergeError::NoCombinedChange);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged(base: &str, local: &str, disk: &str, expected: &str) {
        assert_eq!(
            merge_separate_changes(base, local, disk),
            Ok(expected.into())
        );
        assert_eq!(
            merge_separate_changes(base, disk, local),
            Ok(expected.into())
        );
    }

    #[test]
    fn merges_separated_replacements_in_either_order() {
        merged("a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n", "A\nb\nC\n");
        merged("a\nb\nc", "A\nb\nc", "a\nb\nC", "A\nb\nC");
    }

    #[test]
    fn merges_separated_insertions_deletions_and_replacements() {
        merged(
            "a\nb\nc\n",
            "x\na\nb\nc\n",
            "a\nb\nc\ny\n",
            "x\na\nb\nc\ny\n",
        );
        merged("a\nb\nc\n", "b\nc\n", "a\nb\n", "b\n");
        merged("a\nb\nc\n", "a\nx\nb\nc\n", "a\nb\nC\n", "a\nx\nb\nC\n");
        merged("a\nb\nc\n", "b\nc\n", "a\nb\nC\n", "b\nC\n");
        merged("a\nb\nc\n", "a\nb\n", "x\na\nb\nc\n", "x\na\nb\n");
    }

    #[test]
    fn one_unchanged_line_is_enough_for_a_strict_gap() {
        merged("gap\n", "before\ngap\n", "gap\nafter", "before\ngap\nafter");
        merged("\n", "first\n\n", "\nlast", "first\n\nlast");
    }

    #[test]
    fn preserves_exact_crlf_unicode_and_final_newline_choices() {
        merged(
            "旧😀\r\n中\r\n末\r\n",
            "新🦀\r\n中\r\n末\r\n",
            "旧😀\r\n中\r\n終",
            "新🦀\r\n中\r\n終",
        );
        merged("a\r\nb\nc\r", "A\r\nb\nc\r", "a\r\nb\nC\r", "A\r\nb\nC\r");
        merged("a\nb\nc", "a\r\nb\nc", "a\nb\nc\n", "a\r\nb\nc\n");
        merged(
            "é\nkeep\n😀",
            "e\u{301}\nkeep\n😀",
            "é\nkeep\n🦀",
            "e\u{301}\nkeep\n🦀",
        );
    }

    #[test]
    fn refuses_overlapping_or_touching_changes_and_boundary_insertions() {
        for (base, local, disk) in [
            ("a\nb\nc\n", "A\nb\nc\n", "A2\nb\nc\n"),
            ("a\nb\nc\n", "A\nb\nc\n", "a\nB\nc\n"),
            ("a\nb\nc\n", "b\nc\n", "a\nc\n"),
            ("a\nb\nc\n", "a\nx\nb\nc\n", "a\ny\nb\nc\n"),
            ("a\nb\nc\n", "A\nb\nc\n", "a\nx\nb\nc\n"),
            ("a\nb\nc\n", "a\nB\nc\n", "a\nx\nb\nc\n"),
            ("a\nb\nc\n", "x\na\nb\nc\n", "A\nb\nc\n"),
            ("a\nb\nc\n", "a\nb\nc\nx\n", "a\nb\nC\n"),
            ("", "x\n", "y\n"),
            ("a\nb\nc\n", "", "a\nb\nC\n"),
        ] {
            assert_eq!(
                merge_separate_changes(base, local, disk),
                Err(MergeError::TouchingOrOverlapping)
            );
            assert_eq!(
                merge_separate_changes(base, disk, local),
                Err(MergeError::TouchingOrOverlapping)
            );
        }
    }

    #[test]
    fn multiple_edits_on_a_side_use_one_contiguous_envelope() {
        // The middle local line happens to be unchanged, but remains inside
        // its envelope and cannot be used as a hole for the disk change.
        assert_eq!(
            merge_separate_changes("a\nb\nc\nd\ne\n", "A\nb\nc\nd\nE\n", "a\nb\nC\nd\ne\n"),
            Err(MergeError::TouchingOrOverlapping)
        );
        merged(
            "a\nb\nc\nd\ne\n",
            "A\nb\nC\nd\ne\n",
            "a\nb\nc\nd\nE\n",
            "A\nb\nC\nd\nE\n",
        );
    }

    #[test]
    fn refuses_repeated_line_alignment_instead_of_clamping_the_suffix() {
        for (base, ambiguous, other) in [
            ("a\na\ngap\nz\n", "a\na\na\ngap\nz\n", "a\na\ngap\nZ\n"),
            ("a\na\ngap\nz\n", "a\ngap\nz\n", "a\na\ngap\nZ\n"),
            ("z\ngap\na\na\n", "z\ngap\na\na\na\n", "Z\ngap\na\na\n"),
            ("z\ngap\na\na\n", "z\ngap\na\n", "Z\ngap\na\na\n"),
            ("a\na\na\n", "a\na\n", "a\na\nZ\n"),
            ("a\na\n", "a\na\na\n", "Z\na\n"),
        ] {
            assert_eq!(
                merge_separate_changes(base, ambiguous, other),
                Err(MergeError::AmbiguousAlignment)
            );
            assert_eq!(
                merge_separate_changes(base, other, ambiguous),
                Err(MergeError::AmbiguousAlignment)
            );
        }
    }

    #[test]
    fn refuses_unchanged_sides_and_identical_changes() {
        for (base, local, disk) in [
            ("a\n", "a\n", "A\n"),
            ("a\n", "A\n", "a\n"),
            ("a\n", "a\n", "a\n"),
            ("", "", "x"),
            ("", "x", ""),
            ("", "", ""),
        ] {
            assert_eq!(
                merge_separate_changes(base, local, disk),
                Err(MergeError::UnchangedSide)
            );
        }
        assert_eq!(
            merge_separate_changes("a\n", "A\n", "A\n"),
            Err(MergeError::TouchingOrOverlapping)
        );
    }

    #[test]
    fn refuses_nul_and_oversized_inputs_in_every_position_without_mutating_them() {
        for (invalid, expected) in [
            ("a\0b\n".to_owned(), MergeError::ContainsNul),
            ("x".repeat(MAX_BYTES + 1), MergeError::InputTooLarge),
            ("\n".repeat(MAX_LINES + 1), MergeError::TooManyInputLines),
            (
                format!("{}x", "\n".repeat(MAX_LINES)),
                MergeError::TooManyInputLines,
            ),
        ] {
            for position in 0..3 {
                let mut inputs = [
                    "a\ngap\nz\n".to_owned(),
                    "A\ngap\nz\n".to_owned(),
                    "a\ngap\nZ\n".to_owned(),
                ];
                inputs[position] = invalid.clone();
                let original = inputs.clone();
                assert_eq!(
                    merge_separate_changes(&inputs[0], &inputs[1], &inputs[2]),
                    Err(expected)
                );
                assert_eq!(inputs, original);
            }
        }
    }

    #[test]
    fn accepts_exact_byte_limit_and_refuses_combined_byte_overflow() {
        let tail = "z".repeat(MAX_BYTES - "a\nkeep\n".len());
        let base = format!("a\nkeep\n{tail}");
        let local = format!("A\nkeep\n{tail}");
        let disk = format!("a\nkeep\nZ{}", &tail[1..]);
        let expected = format!("A\nkeep\nZ{}", &tail[1..]);
        merged(&base, &local, &disk, &expected);
        assert_eq!(expected.len(), MAX_BYTES);

        let local = format!("{}\nkeep\n", "x".repeat(MAX_BYTES - 6));
        assert_eq!(local.len(), MAX_BYTES);
        assert_eq!(
            merge_separate_changes("keep\n", &local, "keep\nz\n"),
            Err(MergeError::ResultTooLarge)
        );
    }

    #[test]
    fn accepts_exact_line_limit_and_refuses_combined_line_overflow() {
        let middle = "keep\n".repeat(MAX_LINES - 2);
        let base = format!("a\n{middle}z\n");
        let local = format!("A\n{middle}z\n");
        let disk = format!("a\n{middle}Z\n");
        let expected = format!("A\n{middle}Z\n");
        merged(&base, &local, &disk, &expected);
        assert_eq!(expected.split_inclusive('\n').count(), MAX_LINES);

        let local = format!("{}keep\n", "x\n".repeat(MAX_LINES - 1));
        assert_eq!(local.split_inclusive('\n').count(), MAX_LINES);
        assert_eq!(
            merge_separate_changes("keep\n", &local, "keep\nz\n"),
            Err(MergeError::TooManyResultLines)
        );
        assert_eq!(
            merge_separate_changes("keep\n", &local, "keep\nz"),
            Err(MergeError::TooManyResultLines)
        );
    }

    // An independent, deliberately small single-envelope reference model. It
    // enumerates every envelope and requires one minimum removed+inserted token
    // cost; tied placements are ambiguous. This is not a general diff oracle.
    // Composing token slices also checks the production byte-boundary mapping.
    fn reference(base: &str, local: &str, disk: &str) -> Option<String> {
        let base: Vec<_> = base.split_inclusive('\n').collect();
        let local_tokens: Vec<_> = local.split_inclusive('\n').collect();
        let disk_tokens: Vec<_> = disk.split_inclusive('\n').collect();
        let envelope = |side: &[&str]| {
            if base.as_slice() == side {
                return None;
            }
            let bound = base.len().min(side.len());
            let mut best_cost = usize::MAX;
            let mut best = None;
            let mut tied = false;
            for prefix in 0..=bound {
                for suffix in 0..=bound - prefix {
                    if base[..prefix] != side[..prefix]
                        || base[base.len() - suffix..] != side[side.len() - suffix..]
                    {
                        continue;
                    }
                    let cost = base.len() + side.len() - 2 * (prefix + suffix);
                    if cost < best_cost {
                        best_cost = cost;
                        best = Some((prefix, base.len() - suffix, side.len() - suffix));
                        tied = false;
                    } else if cost == best_cost {
                        tied = true;
                    }
                }
            }
            if tied {
                None
            } else {
                best
            }
        };
        let left = envelope(&local_tokens)?;
        let right = envelope(&disk_tokens)?;
        let (first, first_tokens, second, second_tokens) = if left.1 < right.0 {
            (left, &local_tokens, right, &disk_tokens)
        } else if right.1 < left.0 {
            (right, &disk_tokens, left, &local_tokens)
        } else {
            return None;
        };
        let result = [
            &base[..first.0],
            &first_tokens[first.0..first.2],
            &base[first.1..second.0],
            &second_tokens[second.0..second.2],
            &base[second.1..],
        ]
        .concat()
        .concat();
        (result != local && result != disk).then_some(result)
    }

    #[test]
    fn exhaustive_small_repeated_line_inputs_match_reference() {
        let mut texts = vec![String::new()];
        for length in 1..=4 {
            for bits in 0..(1 << length) {
                let mut text = String::new();
                for index in 0..length {
                    text.push_str(if bits & (1 << index) == 0 {
                        "a\n"
                    } else {
                        "😀\r\n"
                    });
                }
                texts.push(text);
            }
        }
        for base in &texts {
            for local in &texts {
                for disk in &texts {
                    let expected = reference(base, local, disk);
                    assert_eq!(
                        merge_separate_changes(base, local, disk).ok(),
                        expected,
                        "base={base:?}, local={local:?}, disk={disk:?}"
                    );
                }
            }
        }
    }
}
