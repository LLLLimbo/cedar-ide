//! Security for newly created, ephemeral capture pipes only.
//!
//! A pipe's default descriptor grants Everyone/anonymous read access. Never use
//! that default here, nor fall back to the account SID when no logon SID exists.
//! A logon-only DACL is not an isolation boundary against another process in the
//! same logon session; pipe creation additionally verifies its own client PID.

use super::handles::{adopt, raw};
use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::ptr::{null_mut, NonNull};
use windows_sys::Win32::Foundation::{LocalFree, ERROR_INSUFFICIENT_BUFFER};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, IsValidSid, TokenLogonSid, PSID, SECURITY_ATTRIBUTES, TOKEN_GROUPS,
    TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Owns only a LocalAlloc allocation, never a kernel handle or credential.
struct LocalAllocation(NonNull<c_void>);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: Windows returned this allocation for this owner; nothing
        // borrows it after the owner is dropped. LocalFree is its paired free.
        unsafe { LocalFree(self.0.as_ptr()) };
    }
}

pub(crate) struct PipeSecurity {
    descriptor: LocalAllocation,
}

impl PipeSecurity {
    pub(crate) fn for_current_logon() -> io::Result<Self> {
        let sid = current_logon_sid()?;
        // P protects the DACL from inheritance. The sole allow ACE names this
        // logon, not Everyone, Authenticated Users, the account, or all admins.
        // No SYSTEM ACE is needed: the opening and writing handles belong to
        // this logon. This descriptor never changes an existing object.
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
            .encode_utf16()
            .chain([0])
            .collect();
        let mut descriptor = null_mut();
        // SAFETY: SDDL is NUL-terminated and output storage is writable. The
        // successful output is an independent, self-relative allocation.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let descriptor = NonNull::new(descriptor)
            .ok_or_else(|| io::Error::other("Windows returned a null pipe security descriptor"))?;
        Ok(Self {
            descriptor: LocalAllocation(descriptor),
        })
    }

    /// The returned descriptor pointer is borrowed only for CreateNamedPipeW.
    pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0.as_ptr(),
            bInheritHandle: 0,
        }
    }
}

fn current_logon_sid() -> io::Result<String> {
    let mut token = null_mut();
    // SAFETY: GetCurrentProcess is a borrowed pseudo-handle. OpenProcessToken
    // creates one real owned query-only token; it creates no access grant.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: token was freshly returned by successful OpenProcessToken.
    let token = unsafe { adopt(token)? };
    let mut needed = 0;
    // SAFETY: the empty first call queries size only, with writable output.
    let first =
        unsafe { GetTokenInformation(raw(&token), TokenLogonSid, null_mut(), 0, &mut needed) };
    let error = io::Error::last_os_error();
    if first != 0 || error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
        return Err(if first == 0 { error } else { invalid_logon() });
    }
    if (needed as usize) < size_of::<TOKEN_GROUPS>() || needed > 64 * 1024 {
        return Err(invalid_logon());
    }
    // usize storage provides TOKEN_GROUPS/SID_AND_ATTRIBUTES alignment. The
    // token API writes a header and SID pointers into this stable allocation.
    let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    let capacity = needed;
    // SAFETY: storage is aligned, initialized and at least capacity bytes long.
    if unsafe {
        GetTokenInformation(
            raw(&token),
            TokenLogonSid,
            storage.as_mut_ptr().cast(),
            capacity,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if needed > capacity || (needed as usize) < size_of::<TOKEN_GROUPS>() {
        return Err(invalid_logon());
    }
    // SAFETY: successful TokenLogonSid returned the checked TOKEN_GROUPS header.
    let groups = unsafe { &*storage.as_ptr().cast::<TOKEN_GROUPS>() };
    if groups.GroupCount != 1 {
        return Err(invalid_logon());
    }
    let sid = groups.Groups[0].Sid;
    let start = storage.as_ptr() as usize;
    let end = start + needed as usize;
    let address = sid as usize;
    // SID's revision/count/authority header is eight bytes; validate its span
    // before IsValidSid reads the subauthorities. SID data remains in storage.
    if address < start || address.checked_add(8).is_none_or(|n| n > end) {
        return Err(invalid_logon());
    }
    // SAFETY: header span was checked above; SubAuthorityCount is byte one.
    let subauthorities = unsafe { *sid.cast::<u8>().add(1) } as usize;
    if address
        .checked_add(8 + subauthorities * 4)
        .is_none_or(|n| n > end)
        || unsafe { IsValidSid(sid) } == 0
    {
        return Err(invalid_logon());
    }
    // SAFETY: the full SID span and its validity were checked above. Storage
    // stays alive until conversion has copied the string.
    let text = unsafe { sid_string(sid)? };
    if !is_logon_sid(&text) {
        return Err(invalid_logon());
    }
    Ok(text)
}

/// # Safety
/// `sid` must point to a valid SID that remains readable for this call.
unsafe fn sid_string(sid: PSID) -> io::Result<String> {
    let mut string = null_mut();
    // SAFETY: the validated SID remains alive. Successful conversion returns a
    // NUL-terminated UTF-16 LocalAlloc string, retained until copied below.
    if unsafe { ConvertSidToStringSidW(sid, &mut string) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let allocation = LocalAllocation(NonNull::new(string.cast()).ok_or_else(invalid_logon)?);
    let mut len = 0;
    // SAFETY: successful conversion guarantees a terminated string. We read
    // only through that terminator, while its allocation is owned above.
    while unsafe { *string.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: the preceding scan established the valid initialized string span.
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(string, len) })
        .map_err(|_| invalid_logon())?;
    drop(allocation);
    Ok(text)
}

fn invalid_logon() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "a safe current logon SID is required for capture pipes",
    )
}

fn is_logon_sid(sid: &str) -> bool {
    let Some(tail) = sid.strip_prefix("S-1-5-5-") else {
        return false;
    };
    let mut parts = tail.split('-');
    let valid_part = |part: Option<&str>| {
        part.is_some_and(|s| {
            !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && s.parse::<u32>().is_ok()
        })
    };
    valid_part(parts.next()) && valid_part(parts.next()) && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::{
        GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, IsValidSecurityDescriptor,
        ACCESS_ALLOWED_ACE, SE_DACL_PROTECTED,
    };

    #[test]
    fn logon_sid_validation_never_accepts_broad_or_account_identity() {
        assert!(is_logon_sid("S-1-5-5-123-4294967295"));
        for sid in [
            "S-1-1-0",
            "S-1-5-18",
            "S-1-5-21-1-2-3-1001",
            "S-1-5-5-1",
            "S-1-5-5-1-2-3",
            "S-1-5-5-1-4294967296",
            "S-1-5-5-+1-2",
            "S-1-5-5-1-2)(A;;GA;;;WD)",
        ] {
            assert!(!is_logon_sid(sid), "{sid}");
        }
    }

    #[test]
    fn pipe_descriptor_is_protected_and_has_one_explicit_allowance() {
        let security = PipeSecurity::for_current_logon().unwrap();
        let attrs = security.attributes();
        assert_eq!(attrs.bInheritHandle, 0);
        let mut control = 0;
        let mut revision = 0;
        let mut present = 0;
        let mut defaulted = 1;
        let mut acl = null_mut();
        let mut ace = null_mut();
        // SAFETY: descriptor and output fields are live for all inspection.
        unsafe {
            assert_ne!(IsValidSecurityDescriptor(attrs.lpSecurityDescriptor), 0);
            assert_ne!(
                GetSecurityDescriptorControl(
                    attrs.lpSecurityDescriptor,
                    &mut control,
                    &mut revision
                ),
                0
            );
            assert_ne!(
                GetSecurityDescriptorDacl(
                    attrs.lpSecurityDescriptor,
                    &mut present,
                    &mut acl,
                    &mut defaulted
                ),
                0
            );
            assert_ne!(present, 0);
            assert_eq!(defaulted, 0);
            assert!(!acl.is_null());
            assert_eq!((*acl).AceCount, 1);
            assert_ne!(GetAce(acl, 0, &mut ace), 0);
            let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
            // ACCESS_ALLOWED_ACE_TYPE is zero in WinNT.h. This descriptor is
            // from the checked SDDL conversion and must contain that ACE form.
            assert_eq!((*allowed).Header.AceType, 0);
            assert_eq!((*allowed).Header.AceFlags, 0);
            assert_eq!((*allowed).Mask, windows_sys::Win32::Foundation::GENERIC_ALL);
            let sid = std::ptr::addr_of_mut!((*allowed).SidStart).cast();
            assert_eq!(sid_string(sid).unwrap(), current_logon_sid().unwrap());
        }
        assert_ne!(control & SE_DACL_PROTECTED, 0);
    }
}
