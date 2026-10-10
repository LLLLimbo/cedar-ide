//! Memory-only, deliberately conservative candidate. No filesystem operations.
//! Only ordinary allow/deny ACEs are understood. Inheritance flags do not excuse
//! an allow, and denies never subtract a foreign allow. This is not AccessCheck.

pub const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;
pub const MAX_ACES: usize = 128;
const HEADER: usize = 20;
const SELF_RELATIVE: u16 = 0x8000;
const DACL_PRESENT: u16 = 0x0004;
const SYSTEM: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
const ADMINISTRATORS: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    DescriptorLimit,
    MalformedDescriptor,
    MalformedSid,
    MalformedAcl,
    AceLimit,
}

impl ParseError {
    pub fn category(self) -> &'static str {
        match self {
            Self::DescriptorLimit => "descriptor_limit",
            Self::MalformedDescriptor => "malformed_descriptor",
            Self::MalformedSid => "malformed_sid",
            Self::MalformedAcl => "malformed_acl",
            Self::AceLimit => "ace_limit",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    OwnerMismatch,
    AbsentDacl,
    NullDacl,
    EmptyDacl,
    UnsupportedAce,
    ForeignAllow,
    NoAllow,
    NotDisk,
    WrongType,
    Reparse,
    LinkCount,
    PersistentAcl,
    NonemptyFile,
}

impl Rejection {
    pub fn category(self) -> &'static str {
        match self {
            Self::OwnerMismatch => "owner_mismatch",
            Self::AbsentDacl => "absent_dacl",
            Self::NullDacl => "null_dacl",
            Self::EmptyDacl => "empty_dacl",
            Self::UnsupportedAce => "unsupported_ace",
            Self::ForeignAllow => "foreign_allow",
            Self::NoAllow => "no_allow",
            Self::NotDisk => "not_disk",
            Self::WrongType => "wrong_type",
            Self::Reparse => "reparse",
            Self::LinkCount => "link_count",
            Self::PersistentAcl => "persistent_acl_unavailable",
            Self::NonemptyFile => "nonempty_file",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub aces: usize,
    pub allow: usize,
    pub deny: usize,
    pub inherited: usize,
    pub inherit_only: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub rejection: Option<Rejection>,
    pub owner_matches: bool,
    pub counts: Counts,
}

impl Verdict {
    pub fn category(self) -> &'static str {
        self.rejection.map_or("accepted", Rejection::category)
    }
}

#[derive(Clone, Copy)]
pub struct ObjectFacts {
    pub disk: bool,
    pub directory: bool,
    pub reparse: bool,
    pub links: u32,
    pub persistent_acls: bool,
    pub empty: bool,
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, ParseError> {
    let value = bytes
        .get(at..at + 2)
        .ok_or(ParseError::MalformedDescriptor)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ParseError> {
    let value = bytes
        .get(at..at + 4)
        .ok_or(ParseError::MalformedDescriptor)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

/// Used only on bounded slices, including the TokenUser allocation. A SID has
/// revision one and at most fifteen little-endian subauthorities (68 bytes).
pub fn sid_prefix(bytes: &[u8]) -> Result<&[u8], ParseError> {
    if bytes.len() < 8 || bytes[0] != 1 || bytes[1] > 15 {
        return Err(ParseError::MalformedSid);
    }
    bytes
        .get(..8 + usize::from(bytes[1]) * 4)
        .ok_or(ParseError::MalformedSid)
}

fn offset(bytes: &[u8], field: usize) -> Result<usize, ParseError> {
    let value = u32_at(bytes, field)? as usize;
    if value != 0 && (value < HEADER || !value.is_multiple_of(4) || value >= bytes.len()) {
        return Err(ParseError::MalformedDescriptor);
    }
    Ok(value)
}

pub fn assess(
    bytes: &[u8],
    effective_user: &[u8],
    facts: ObjectFacts,
    expect_directory: bool,
) -> Result<Verdict, ParseError> {
    if bytes.len() > MAX_DESCRIPTOR_BYTES {
        return Err(ParseError::DescriptorLimit);
    }
    if bytes.len() < HEADER || bytes[0] != 1 || bytes[1] != 0 {
        return Err(ParseError::MalformedDescriptor);
    }
    if sid_prefix(effective_user)?.len() != effective_user.len() {
        return Err(ParseError::MalformedSid);
    }
    let control = u16_at(bytes, 2)?;
    // The native query requests OWNER + DACL only. A SACL is outside this
    // parser's contract, so never silently interpret an unexpected one.
    if control & SELF_RELATIVE == 0 || control & 0x0010 != 0 || u32_at(bytes, 12)? != 0 {
        return Err(ParseError::MalformedDescriptor);
    }
    let owner_at = offset(bytes, 4)?;
    if owner_at == 0 {
        return Err(ParseError::MalformedDescriptor);
    }
    let owner = sid_prefix(&bytes[owner_at..])?;
    let group_at = offset(bytes, 8)?;
    let group_end = if group_at == 0 {
        0
    } else {
        group_at + sid_prefix(&bytes[group_at..])?.len()
    };
    let dacl_at = offset(bytes, 16)?;
    let mut result = Verdict {
        rejection: None,
        owner_matches: owner == effective_user,
        counts: Counts::default(),
    };
    if control & DACL_PRESENT == 0 {
        if dacl_at != 0 {
            return Err(ParseError::MalformedDescriptor);
        }
        result.rejection = Some(Rejection::AbsentDacl);
    } else if dacl_at == 0 {
        result.rejection = Some(Rejection::NullDacl);
    } else {
        let tail = &bytes[dacl_at..];
        if tail.len() < 8 || !matches!(tail[0], 2 | 4) || tail[1] != 0 || tail[6..8] != [0, 0] {
            return Err(ParseError::MalformedAcl);
        }
        let size = usize::from(u16_at(tail, 2)?);
        let acl = tail
            .get(..size)
            .filter(|a| a.len() >= 8 && a.len().is_multiple_of(4))
            .ok_or(ParseError::MalformedAcl)?;
        let overlaps = |start: usize, end: usize| start < dacl_at + size && end > dacl_at;
        if overlaps(owner_at, owner_at + owner.len())
            || (group_at != 0 && overlaps(group_at, group_end))
        {
            return Err(ParseError::MalformedDescriptor);
        }
        let count = usize::from(u16_at(acl, 4)?);
        if count > MAX_ACES {
            return Err(ParseError::AceLimit);
        }
        let mut cursor = 8;
        let mut foreign_allow = false;
        let mut unsupported = false;
        for _ in 0..count {
            let header = acl
                .get(cursor..cursor + 4)
                .ok_or(ParseError::MalformedAcl)?;
            let ace_size = usize::from(u16::from_le_bytes([header[2], header[3]]));
            if ace_size < 4 || !ace_size.is_multiple_of(4) {
                return Err(ParseError::MalformedAcl);
            }
            let ace = acl
                .get(cursor..cursor + ace_size)
                .ok_or(ParseError::MalformedAcl)?;
            cursor += ace_size;
            result.counts.aces += 1;
            result.counts.inherited += usize::from(header[1] & 0x10 != 0);
            result.counts.inherit_only += usize::from(header[1] & 0x08 != 0);
            unsupported |= header[1] & !0x1f != 0;
            if !matches!(header[0], 0 | 1) {
                unsupported = true;
                continue;
            }
            let sid_bytes = ace.get(8..).ok_or(ParseError::MalformedAcl)?;
            let trustee = sid_prefix(sid_bytes)?;
            if trustee.len() != sid_bytes.len() {
                return Err(ParseError::MalformedAcl);
            }
            if header[0] == 0 {
                result.counts.allow += 1;
                foreign_allow |=
                    trustee != effective_user && trustee != SYSTEM && trustee != ADMINISTRATORS;
            } else {
                result.counts.deny += 1;
            }
        }
        // ACL allocation slack is permitted; it has no ACE semantics. Traversal
        // is governed by AceCount and AclSize, never by searching slack bytes.
        result.rejection = if foreign_allow {
            Some(Rejection::ForeignAllow)
        } else if unsupported {
            Some(Rejection::UnsupportedAce)
        } else if count == 0 {
            Some(Rejection::EmptyDacl)
        } else if result.counts.allow == 0 {
            Some(Rejection::NoAllow)
        } else {
            None
        };
    }
    if owner != effective_user {
        result.rejection = Some(Rejection::OwnerMismatch);
    }
    // Directory link counts are filesystem-specific; files must be single-link.
    let object_rejection = if !facts.disk {
        Some(Rejection::NotDisk)
    } else if facts.reparse {
        Some(Rejection::Reparse)
    } else if facts.directory != expect_directory {
        Some(Rejection::WrongType)
    } else if !facts.directory && facts.links != 1 {
        Some(Rejection::LinkCount)
    } else if !facts.persistent_acls {
        Some(Rejection::PersistentAcl)
    } else if !facts.directory && !facts.empty {
        Some(Rejection::NonemptyFile)
    } else {
        None
    };
    result.rejection = object_rejection.or(result.rejection);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 42, 0, 0, 0];
    const FOREIGN: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];

    fn facts() -> ObjectFacts {
        ObjectFacts {
            disk: true,
            directory: false,
            reparse: false,
            links: 1,
            persistent_acls: true,
            empty: true,
        }
    }

    fn ace(kind: u8, flags: u8, sid: &[u8]) -> Vec<u8> {
        let size = 8 + sid.len();
        let mut bytes = vec![kind, flags];
        bytes.extend_from_slice(&(size as u16).to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(sid);
        bytes
    }

    fn descriptor(owner: &[u8], aces: &[Vec<u8>]) -> Vec<u8> {
        let dacl_at = HEADER + owner.len();
        let mut bytes = vec![1, 0, 4, 128];
        bytes.extend_from_slice(&(HEADER as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&(dacl_at as u32).to_le_bytes());
        bytes.extend_from_slice(owner);
        let size = 8 + aces.iter().map(Vec::len).sum::<usize>();
        bytes.extend_from_slice(&[2, 0]);
        bytes.extend_from_slice(&(size as u16).to_le_bytes());
        bytes.extend_from_slice(&(aces.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        for ace in aces {
            bytes.extend_from_slice(ace);
        }
        bytes
    }

    fn evaluate(bytes: &[u8]) -> Result<Verdict, ParseError> {
        assess(bytes, USER, facts(), false)
    }

    #[test]
    fn accepts_only_exact_user_system_and_administrators_ordinary_allows() {
        for trustee in [USER, SYSTEM, ADMINISTRATORS] {
            let result = evaluate(&descriptor(USER, &[ace(0, 0x1f, trustee)])).unwrap();
            assert_eq!(result.category(), "accepted");
            assert!(result.owner_matches);
            assert_eq!(
                result.counts,
                Counts {
                    aces: 1,
                    allow: 1,
                    inherited: 1,
                    inherit_only: 1,
                    deny: 0
                }
            );
        }
        assert_eq!(
            evaluate(&descriptor(SYSTEM, &[ace(0, 0, USER)]))
                .unwrap()
                .rejection,
            Some(Rejection::OwnerMismatch)
        );
        let mut similar = USER.to_vec();
        similar[12] += 1;
        assert_eq!(
            evaluate(&descriptor(USER, &[ace(0, 0, &similar)]))
                .unwrap()
                .rejection,
            Some(Rejection::ForeignAllow)
        );
    }

    #[test]
    fn inherited_and_inherit_only_foreign_allows_reject_despite_deny_order_or_zero_mask() {
        for flags in [0, 0x10, 0x08, 0x18] {
            for deny_first in [false, true] {
                let mut allow = ace(0, flags, FOREIGN);
                allow[4..8].fill(0); // Even a zero mask never exempts a foreign allow.
                let deny = ace(1, 0, FOREIGN);
                let aces = if deny_first {
                    vec![deny, allow]
                } else {
                    vec![allow, deny]
                };
                assert_eq!(
                    evaluate(&descriptor(USER, &aces)).unwrap().rejection,
                    Some(Rejection::ForeignAllow)
                );
            }
        }
        assert_eq!(
            evaluate(&descriptor(USER, &[ace(1, 0, FOREIGN)]))
                .unwrap()
                .rejection,
            Some(Rejection::NoAllow)
        );
    }

    #[test]
    fn null_absent_empty_and_unknown_dacls_fail_closed() {
        let mut bytes = descriptor(USER, &[]);
        assert_eq!(evaluate(&bytes).unwrap().category(), "empty_dacl");
        bytes[16..20].fill(0);
        assert_eq!(evaluate(&bytes).unwrap().category(), "null_dacl");
        bytes[2] = 0;
        assert_eq!(evaluate(&bytes).unwrap().category(), "absent_dacl");
        for kind in [2, 5, 6, 9, 10, 11, 12, 17, 255] {
            assert_eq!(
                evaluate(&descriptor(USER, &[ace(kind, 0, USER)]))
                    .unwrap()
                    .category(),
                "unsupported_ace"
            );
        }
        assert_eq!(
            evaluate(&descriptor(USER, &[ace(0, 0x40, USER)]))
                .unwrap()
                .category(),
            "unsupported_ace"
        );
    }

    #[test]
    fn malformed_descriptor_sid_acl_and_bounds_fail_closed_without_panics() {
        let valid = descriptor(USER, &[ace(0, 0, USER)]);
        for end in 0..valid.len() {
            assert!(evaluate(&valid[..end]).is_err());
        }
        for field in [4, 8, 12, 16] {
            let mut bytes = valid.clone();
            bytes[field..field + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(evaluate(&bytes).is_err());
        }
        for (at, value) in [
            (0, 2),
            (1, 1),
            (3, 0),
            (20, 2),
            (21, 16),
            (36, 1),
            (37, 1),
            (38, 7),
            (42, 1),
            (46, 3),
            (53, 16),
        ] {
            let mut bytes = valid.clone();
            bytes[at] = value;
            assert!(evaluate(&bytes).is_err(), "fixed mutation index {at}");
        }
        let mut overlap = valid.clone();
        overlap[4..8].copy_from_slice(&44u32.to_le_bytes());
        assert!(evaluate(&overlap).is_err());
        assert_eq!(
            evaluate(&vec![0; MAX_DESCRIPTOR_BYTES + 1])
                .unwrap_err()
                .category(),
            "descriptor_limit"
        );
        let too_many = descriptor(USER, &vec![ace(0, 0, USER); MAX_ACES + 1]);
        assert_eq!(evaluate(&too_many).unwrap_err().category(), "ace_limit");
        let at_limit = descriptor(USER, &vec![ace(0, 0, USER); MAX_ACES]);
        assert_eq!(evaluate(&at_limit).unwrap().counts.aces, MAX_ACES);
        let mut padded = valid.clone();
        padded.resize(MAX_DESCRIPTOR_BYTES, 0);
        assert!(evaluate(&padded).unwrap().rejection.is_none());
    }

    #[test]
    fn unsafe_object_facts_fail_closed() {
        let bytes = descriptor(USER, &[ace(0, 0, USER)]);
        for (bad, expected) in [
            (
                ObjectFacts {
                    disk: false,
                    ..facts()
                },
                Rejection::NotDisk,
            ),
            (
                ObjectFacts {
                    directory: true,
                    ..facts()
                },
                Rejection::WrongType,
            ),
            (
                ObjectFacts {
                    reparse: true,
                    ..facts()
                },
                Rejection::Reparse,
            ),
            (
                ObjectFacts {
                    links: 0,
                    ..facts()
                },
                Rejection::LinkCount,
            ),
            (
                ObjectFacts {
                    links: 2,
                    ..facts()
                },
                Rejection::LinkCount,
            ),
            (
                ObjectFacts {
                    persistent_acls: false,
                    ..facts()
                },
                Rejection::PersistentAcl,
            ),
            (
                ObjectFacts {
                    empty: false,
                    ..facts()
                },
                Rejection::NonemptyFile,
            ),
        ] {
            assert_eq!(
                assess(&bytes, USER, bad, false).unwrap().rejection,
                Some(expected)
            );
        }
    }

    #[test]
    fn fake_sink_receives_zero_metadata_and_body_bytes_on_every_rejection_or_error() {
        #[derive(Default)]
        struct FakeSink {
            metadata: usize,
            body: usize,
        }
        fn gated_emit<E>(result: Result<Verdict, E>, sink: &mut FakeSink) {
            if matches!(
                result,
                Ok(Verdict {
                    rejection: None,
                    ..
                })
            ) {
                sink.metadata += b"memory-only-metadata".len();
                sink.body += b"memory-only-body".len();
            }
        }
        for rejection in [
            Rejection::OwnerMismatch,
            Rejection::AbsentDacl,
            Rejection::NullDacl,
            Rejection::EmptyDacl,
            Rejection::UnsupportedAce,
            Rejection::ForeignAllow,
            Rejection::NoAllow,
            Rejection::NotDisk,
            Rejection::WrongType,
            Rejection::Reparse,
            Rejection::LinkCount,
            Rejection::PersistentAcl,
            Rejection::NonemptyFile,
        ] {
            let mut sink = FakeSink::default();
            gated_emit::<ParseError>(
                Ok(Verdict {
                    rejection: Some(rejection),
                    owner_matches: rejection != Rejection::OwnerMismatch,
                    counts: Counts::default(),
                }),
                &mut sink,
            );
            assert_eq!((sink.metadata, sink.body), (0, 0));
        }
        for error in [
            ParseError::DescriptorLimit,
            ParseError::MalformedDescriptor,
            ParseError::MalformedSid,
            ParseError::MalformedAcl,
            ParseError::AceLimit,
        ] {
            let mut sink = FakeSink::default();
            gated_emit(Err(error), &mut sink);
            assert_eq!((sink.metadata, sink.body), (0, 0));
        }
        // Exercise actual parsing and object-admission composition as well as
        // every terminal category. A native inspection failure also closes it.
        let mut malformed = descriptor(USER, &[ace(0, 0, USER)]);
        malformed[21] = 16;
        for bytes in [
            descriptor(USER, &[ace(0, 0x18, FOREIGN), ace(1, 0, FOREIGN)]),
            descriptor(SYSTEM, &[ace(0, 0, USER)]),
            descriptor(USER, &[]),
            malformed,
        ] {
            let mut sink = FakeSink::default();
            gated_emit(evaluate(&bytes), &mut sink);
            assert_eq!((sink.metadata, sink.body), (0, 0));
        }
        let mut sink = FakeSink::default();
        gated_emit(
            assess(
                &descriptor(USER, &[ace(0, 0, USER)]),
                USER,
                ObjectFacts {
                    reparse: true,
                    ..facts()
                },
                false,
            ),
            &mut sink,
        );
        gated_emit(Err("synthetic_inspection_failure"), &mut sink);
        assert_eq!((sink.metadata, sink.body), (0, 0));
        let mut sink = FakeSink::default();
        gated_emit(evaluate(&descriptor(USER, &[ace(0, 0, USER)])), &mut sink);
        assert!(
            sink.metadata > 0 && sink.body > 0,
            "positive control exercises the fake sink"
        );
    }
}
