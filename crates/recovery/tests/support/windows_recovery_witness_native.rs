//! One opt-in, zero-byte, default-inheritance LocalAppData fixture only.
//! Cleanup identity checks are separate from the sole admission observation.
//! Controlled fresh-runner evidence only: directory creation followed by an
//! open is not atomic and ancestors are not pinned or security-verified. This
//! does not establish race-free namespace ownership or production admission.
use super::queries::{NativeQueries, Retained};
use super::witness::{reject_before_read, Receipt};
use std::fs::{self, OpenOptions};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::*;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity(u64, [u8; 16]);

fn open(path: &Path, delete: bool) -> Option<OwnedHandle> {
    let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
    if name.contains(&0) {
        return None;
    }
    name.push(0);
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_READ_ATTRIBUTES | if delete { 0x0001_0000 } else { 0 },
            if delete {
                FILE_SHARE_READ
            } else {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            },
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    (raw != INVALID_HANDLE_VALUE).then(|| unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn identity(handle: &impl AsRawHandle, directory: bool) -> Option<Identity> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let mut id: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    let mut tag: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut info) } == 0
        || unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        || unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileAttributeTagInfo,
                (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        } == 0
        || unsafe { GetFileType(handle.as_raw_handle()) } != FILE_TYPE_DISK
        || tag.FileAttributes != info.dwFileAttributes
        || tag.ReparseTag != 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory
            && (info.nNumberOfLinks != 1 || info.nFileSizeHigh != 0 || info.nFileSizeLow != 0))
    {
        return None;
    }
    Some(Identity(id.VolumeSerialNumber, id.FileId.Identifier))
}

fn delete_generated(handle: OwnedHandle) -> bool {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let success = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } != 0;
    drop(handle);
    success
}

// Never recurse, discover deletion targets, retry, or remove an object with
// unknown/replaced identity. Uncertainty leaves the fixture and fails receipt.
fn cleanup(
    root: &Path,
    root_id: Option<Identity>,
    file_id: Option<Identity>,
    receipt: &mut Receipt,
) -> bool {
    let Some(root_handle) = open(root, true) else {
        return false;
    };
    if root_id.is_none() || identity(&root_handle, true) != root_id {
        return false;
    }
    if receipt.generated_files == 1 {
        let Some(file) = open(&root.join("empty.record"), true) else {
            return false;
        };
        if file_id.is_none() || identity(&file, false) != file_id || !delete_generated(file) {
            return false;
        }
    }
    receipt.empty_verified = fs::read_dir(root).is_ok_and(|mut entries| entries.next().is_none());
    if !receipt.empty_verified || !delete_generated(root_handle) {
        return false;
    }
    matches!(fs::symlink_metadata(root), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

pub fn run() -> Receipt {
    let mut receipt = Receipt::default();
    if std::env::var("CEDAR_RUN_ADMISSION_WITNESS").as_deref() != Ok("1") {
        receipt.category = "opt_in_required";
        return receipt;
    }
    let mut root = None;
    let mut root_id = None;
    let mut file_id = None;
    let result: Result<(), &'static str> = (|| {
        let base = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("setup_error")?);
        if !base.is_absolute() {
            return Err("setup_error");
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "setup_error")?
            .as_nanos();
        let child = base.join(format!(
            "cedar-admission-witness-{}-{stamp}",
            std::process::id()
        ));
        // One attempt. No tempfile name-selection retries, custom security
        // attributes, ACL/owner writes, privileges, alternate roots, or content.
        fs::create_dir(&child).map_err(|_| "setup_error")?;
        receipt.generated_directories = 1;
        root = Some(child.clone());
        let directory = open(&child, false).ok_or("setup_error")?;
        root_id = Some(identity(&directory, true).ok_or("setup_error")?);
        let path = child.join("empty.record");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "setup_error")?;
        receipt.generated_files = 1;
        file_id = Some(identity(&file, false).ok_or("setup_error")?);
        // Bridge creation to Retained with a read-attributes-only pin. Its
        // missing FILE_SHARE_DELETE prevents replacing the generated file;
        // unlike the creation writer it can coexist with Retained's share mode.
        let file_pin = open(&path, false).ok_or("setup_error")?;
        if identity(&file_pin, false) != file_id {
            return Err("setup_error");
        }
        drop(file);
        let retained = Retained::open_existing(&path).map_err(|_| "query_error")?;
        reject_before_read(retained, &mut NativeQueries, &mut receipt);
        drop(file_pin);
        drop(directory);
        Ok(())
    })();
    if let Err(category) = result {
        receipt.category = category;
    }
    // Closure-local handles, including early-return handles and Admitted, are
    // all dropped before any cleanup handle is opened or deletion attempted.
    receipt.handles_dropped_before_cleanup = true;
    receipt.cleanup_complete = root
        .as_ref()
        .is_none_or(|root| cleanup(root, root_id, file_id, &mut receipt));
    if !receipt.cleanup_complete {
        receipt.category = "cleanup_error";
    }
    receipt
}
