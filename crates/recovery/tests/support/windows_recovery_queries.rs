//! NONSHIPPING read-only Win32 queries. No creation, ACL setters, privileges,
//! payload I/O, rename or deletion. Tests below only inject memory buffers.
use super::admission::{Error, Observation, Queries};
use super::policy::{self, Identity, ObjectFacts, MAX_DESCRIPTOR_BYTES};
use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    GetHandleInformation, GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_TOKEN, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{
    GetKernelObjectSecurity, GetTokenInformation, TokenUser, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

pub struct Retained(OwnedHandle);
impl Retained {
    pub fn open_existing(path: &Path) -> Result<Self, Error> {
        let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
        if name.contains(&0) {
            return Err(Error::Bounds);
        }
        name.push(0);
        // Null SECURITY_ATTRIBUTES creates a noninheritable handle. OPEN_EXISTING
        // never creates an object. Sharing limits conflicting write/delete opens;
        // it does not freeze descriptors or provide an atomic namespace check.
        let raw = unsafe {
            CreateFileW(
                name.as_ptr(),
                0x0002_0000 | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(Error::Query);
        }
        // SAFETY: a successful CreateFileW transfers exactly one owned handle.
        let owned = unsafe { OwnedHandle::from_raw_handle(raw) };
        noninheritable(&owned)?;
        Ok(Self(owned))
    }
}
fn noninheritable(handle: &OwnedHandle) -> Result<(), Error> {
    let mut flags = 0;
    if unsafe { GetHandleInformation(handle.as_raw_handle(), &mut flags) } == 0 {
        return Err(Error::Query);
    }
    validate_handle_flags(Ok(flags))
}
fn validate_handle_flags(flags: Result<u32, Error>) -> Result<(), Error> {
    if flags? & HANDLE_FLAG_INHERIT != 0 {
        return Err(Error::Inheritable);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Reply {
    success: bool,
    error: u32,
    returned: u32,
}
struct Buffer {
    storage: Vec<usize>,
    returned: usize,
}
impl Buffer {
    fn bytes(&self) -> &[u8] {
        // SAFETY: bounded_query validates returned against the initialized,
        // aligned allocation before constructing Buffer. Padding is not exposed.
        unsafe { std::slice::from_raw_parts(self.storage.as_ptr().cast(), self.returned) }
    }
}
fn bounded_query(
    min: usize,
    max: usize,
    mut query: impl FnMut(*mut c_void, u32) -> Reply,
) -> Result<Buffer, Error> {
    let size = query(null_mut(), 0);
    if size.success || size.error != ERROR_INSUFFICIENT_BUFFER {
        return Err(Error::Query);
    }
    let capacity = size.returned as usize;
    if capacity < min || capacity > max {
        return Err(Error::Bounds);
    }
    let mut storage = vec![0usize; capacity.div_ceil(size_of::<usize>())];
    let data = query(storage.as_mut_ptr().cast(), size.returned);
    // No retries, including ERROR_INSUFFICIENT_BUFFER or growth on success.
    if !data.success {
        return Err(Error::Query);
    }
    if (data.returned as usize) < min || data.returned as usize > capacity {
        return Err(Error::Bounds);
    }
    Ok(Buffer {
        storage,
        returned: data.returned as usize,
    })
}
fn token_sid(buffer: &Buffer) -> Result<Vec<u8>, Error> {
    if buffer.returned < size_of::<TOKEN_USER>() {
        return Err(Error::Bounds);
    }
    // SAFETY: usize-aligned allocation and validated complete TOKEN_USER.
    let user = unsafe { &*buffer.storage.as_ptr().cast::<TOKEN_USER>() };
    let offset = (user.User.Sid as usize)
        .checked_sub(buffer.storage.as_ptr() as usize)
        .ok_or(Error::Bounds)?;
    if offset < size_of::<TOKEN_USER>() || offset >= buffer.returned || !offset.is_multiple_of(4) {
        return Err(Error::Bounds);
    }
    policy::sid_prefix(&buffer.bytes()[offset..])
        .map(Vec::from)
        .map_err(|_| Error::Bounds)
}
fn thread_or_process<T>(
    thread: impl FnOnce() -> Result<T, u32>,
    process: impl FnOnce() -> Result<T, u32>,
) -> Result<T, Error> {
    match thread() {
        Ok(token) => Ok(token),
        Err(ERROR_NO_TOKEN) => process().map_err(|_| Error::Query),
        Err(_) => Err(Error::Query),
    }
}
fn open_token(thread: bool) -> Result<OwnedHandle, u32> {
    let mut raw: HANDLE = null_mut();
    let ok = unsafe {
        if thread {
            OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw)
        } else {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw)
        }
    };
    if ok == 0 {
        return Err(unsafe { GetLastError() });
    }
    // SAFETY: successful token-open transfers exactly one owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
fn effective_user() -> Result<Vec<u8>, Error> {
    let token = thread_or_process(|| open_token(true), || open_token(false))?;
    noninheritable(&token)?;
    let buffer = bounded_query(size_of::<TOKEN_USER>(), 4096, |data, capacity| {
        let mut returned = 0;
        let success = unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                data,
                capacity,
                &mut returned,
            )
        } != 0;
        let error = if success {
            0
        } else {
            unsafe { GetLastError() }
        };
        Reply {
            success,
            error,
            returned,
        }
    })?;
    token_sid(&buffer)
}
fn descriptor(handle: HANDLE) -> Result<Vec<u8>, Error> {
    bounded_query(20, MAX_DESCRIPTOR_BYTES, |data, capacity| {
        let mut returned = 0;
        let success = unsafe {
            GetKernelObjectSecurity(
                handle,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                data,
                capacity,
                &mut returned,
            )
        } != 0;
        let error = if success {
            0
        } else {
            unsafe { GetLastError() }
        };
        Reply {
            success,
            error,
            returned,
        }
    })
    .map(|buffer| buffer.bytes().to_vec())
}
fn metadata(handle: HANDLE) -> Result<(Identity, ObjectFacts), Error> {
    let mut basic: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let mut tag: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
    let mut id: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    let mut flags = 0;
    if unsafe { GetFileInformationByHandle(handle, &mut basic) } == 0
        || unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        } == 0
        || unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        || unsafe {
            GetVolumeInformationByHandleW(
                handle,
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                &mut flags,
                null_mut(),
                0,
            )
        } == 0
    {
        return Err(Error::Query);
    }
    if basic.dwFileAttributes != tag.FileAttributes {
        return Err(Error::IdentityChanged);
    }
    Ok((
        Identity {
            volume: id.VolumeSerialNumber,
            file: id.FileId.Identifier,
        },
        ObjectFacts {
            disk: unsafe { GetFileType(handle) } == FILE_TYPE_DISK,
            directory: tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
            reparse: tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || tag.ReparseTag != 0,
            links: basic.nNumberOfLinks,
            persistent_acls: flags & FILE_PERSISTENT_ACLS != 0,
            empty: basic.nFileSizeHigh == 0 && basic.nFileSizeLow == 0,
        },
    ))
}
pub struct NativeQueries;
impl Queries<Retained> for NativeQueries {
    fn observe(&mut self, retained: &Retained) -> Result<Observation, Error> {
        noninheritable(&retained.0)?;
        let user = effective_user()?;
        let handle = retained.0.as_raw_handle();
        let (identity, _) = metadata(handle)?;
        let descriptor = descriptor(handle)?;
        let (after, facts) = metadata(handle)?;
        if after != identity {
            return Err(Error::IdentityChanged);
        }
        if effective_user()? != user {
            return Err(Error::TokenChanged);
        }
        Ok(Observation {
            identity,
            facts,
            descriptor,
            user,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inheritable_or_unqueryable_handles_never_pass() {
        assert_eq!(validate_handle_flags(Ok(0)), Ok(()));
        assert_eq!(
            validate_handle_flags(Ok(HANDLE_FLAG_INHERIT)),
            Err(Error::Inheritable)
        );
        assert_eq!(validate_handle_flags(Err(Error::Query)), Err(Error::Query));
    }
    #[test]
    fn descriptor_buffer_exposes_only_returned_bytes_and_rejects_query_uncertainty() {
        let mut calls = 0;
        let buffer = bounded_query(20, MAX_DESCRIPTOR_BYTES, |data, _| {
            calls += 1;
            if calls == 1 {
                return Reply {
                    success: false,
                    error: ERROR_INSUFFICIENT_BUFFER,
                    returned: 24,
                };
            }
            assert_eq!(data as usize % 4, 0);
            unsafe {
                std::ptr::write_bytes(data.cast::<u8>(), 0xa5, 24);
            }
            Reply {
                success: true,
                error: 0,
                returned: 20,
            }
        })
        .unwrap();
        assert_eq!(buffer.bytes(), &[0xa5; 20]);
        for (success, error) in [(true, 0), (false, 0), (false, 5)] {
            let mut calls = 0;
            assert!(bounded_query(20, MAX_DESCRIPTOR_BYTES, |_, _| {
                calls += 1;
                Reply {
                    success,
                    error,
                    returned: 20,
                }
            })
            .is_err());
            assert_eq!(calls, 1);
        }
    }
    #[test]
    fn native_buffers_are_aligned_bounded_and_never_retried() {
        for (reported, success) in [(31, true), (64, true), (32, false)] {
            let mut calls = 0;
            let result = bounded_query(32, 4096, |data, capacity| {
                calls += 1;
                if calls == 1 {
                    assert!(data.is_null());
                    return Reply {
                        success: false,
                        error: ERROR_INSUFFICIENT_BUFFER,
                        returned: 32,
                    };
                }
                assert_eq!(capacity, 32);
                assert_eq!(data as usize % std::mem::align_of::<TOKEN_USER>(), 0);
                Reply {
                    success,
                    error: ERROR_INSUFFICIENT_BUFFER,
                    returned: reported,
                }
            });
            assert!(result.is_err());
            assert_eq!(calls, 2);
        }
        for size in [0, 19, 65537, u32::MAX] {
            let mut calls = 0;
            assert!(bounded_query(20, 65536, |_, _| {
                calls += 1;
                Reply {
                    success: false,
                    error: ERROR_INSUFFICIENT_BUFFER,
                    returned: size,
                }
            })
            .is_err());
            assert_eq!(calls, 1);
        }
    }
    #[test]
    fn token_pointer_is_validated_before_dereference() {
        for displacement in [0, 1, size_of::<TOKEN_USER>() - 1, 60, 64, usize::MAX] {
            let mut buffer = Buffer {
                storage: vec![0; 64 / size_of::<usize>()],
                returned: 64,
            };
            let start = buffer.storage.as_ptr() as usize;
            let pointer = if displacement == usize::MAX {
                start - 4
            } else {
                start + displacement
            };
            unsafe {
                (*buffer.storage.as_mut_ptr().cast::<TOKEN_USER>()).User.Sid =
                    pointer as *mut c_void;
            }
            assert_eq!(token_sid(&buffer), Err(Error::Bounds));
        }
        let mut buffer = Buffer {
            storage: vec![0; 64 / size_of::<usize>()],
            returned: 64,
        };
        let offset = size_of::<TOKEN_USER>();
        let sid = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
        unsafe {
            let base = buffer.storage.as_mut_ptr().cast::<u8>();
            std::ptr::copy_nonoverlapping(sid.as_ptr(), base.add(offset), sid.len());
            (*base.cast::<TOKEN_USER>()).User.Sid = base.add(offset).cast();
        }
        assert_eq!(token_sid(&buffer).unwrap(), sid);
        buffer.returned = offset + sid.len() - 1;
        assert_eq!(token_sid(&buffer), Err(Error::Bounds));
    }
    #[test]
    fn only_no_token_allows_process_fallback() {
        for error in [0, 5, 122, ERROR_NO_TOKEN] {
            let mut calls = 0;
            let result = thread_or_process(
                || Err(error),
                || {
                    calls += 1;
                    Ok(7)
                },
            );
            assert_eq!(calls, usize::from(error == ERROR_NO_TOKEN));
            assert_eq!(result.is_ok(), error == ERROR_NO_TOKEN);
        }
        assert_eq!(
            thread_or_process(|| Ok(9), || panic!("must not fallback")),
            Ok(9)
        );
    }
}
