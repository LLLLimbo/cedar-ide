use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};

/// Adopt exactly one newly created, non-pseudo Win32 kernel handle.
///
/// # Safety
/// On success `handle` must be newly owned by the caller, not borrowed or already
/// wrapped. The caller must pass the return value before another Win32 call can
/// overwrite the thread's last error. This function takes that ownership.
pub(crate) unsafe fn adopt(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the caller's ownership contract is checked for invalid sentinels.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

pub(crate) fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle()
}

pub(crate) struct ChildStdio {
    pub(crate) stdin: OwnedHandle,
    pub(crate) stdout: OwnedHandle,
    pub(crate) stderr: OwnedHandle,
}
impl ChildStdio {
    pub(crate) fn raw_handles(&self) -> [HANDLE; 3] {
        [raw(&self.stdin), raw(&self.stdout), raw(&self.stderr)]
    }
}
