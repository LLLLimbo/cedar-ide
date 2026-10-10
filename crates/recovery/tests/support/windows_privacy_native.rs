//! Windows-only, test-only observation of newly created empty objects. No Store
//! APIs, descriptor setters, account lookup, privileges, or content writes.
//! The parent driver enforces the hard 60-second process budget. These shared
//! 55-second admission checks cannot interrupt a blocking filesystem call.

use super::policy::{
    self, Identity, IdentityCheck, ObjectFacts, ProbeOpenKind, Verdict, MAX_DESCRIPTOR_BYTES,
};
use serde::Serialize;
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_TOKEN,
    ERROR_PATH_NOT_FOUND, HANDLE, INVALID_HANDLE_VALUE,
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
use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Serialize)]
pub struct Receipt {
    schema_version: u32,
    probe: &'static str,
    descriptor_query: &'static str,
    shipping_unchanged: bool,
    metadata_bytes_written: usize,
    body_bytes_written: usize,
    roots: [RootReport; 2],
}

impl Receipt {
    pub fn succeeded(&self) -> bool {
        self.roots
            .iter()
            .all(|root| root.outcome != "error" && root.cleanup_complete)
    }
}

#[derive(Serialize)]
struct RootReport {
    root: &'static str,
    outcome: &'static str,
    stage: &'static str,
    category: &'static str,
    objects_observed: usize,
    descriptor_reads: usize,
    candidate_accepts: usize,
    candidate_rejections: usize,
    ace_count: usize,
    allow_ace_count: usize,
    deny_ace_count: usize,
    inherited_ace_count: usize,
    inherit_only_ace_count: usize,
    rename_attempted: bool,
    rename_completed: bool,
    rename_error_category: &'static str,
    identity_check: &'static str,
    cleanup_complete: bool,
    objects: Vec<ObjectReport>,
}

#[derive(Serialize)]
struct ObjectReport {
    role: &'static str,
    outcome: &'static str,
    category: &'static str,
    owner_matches: bool,
    owner_category: &'static str,
    dacl_category: &'static str,
    ace_count: usize,
    allow_ace_count: usize,
    deny_ace_count: usize,
    inherited_ace_count: usize,
    inherit_only_ace_count: usize,
}

impl RootReport {
    fn new(root: &'static str) -> Self {
        Self {
            root,
            outcome: "candidate_accepted",
            stage: "setup",
            category: "accepted",
            objects_observed: 0,
            descriptor_reads: 0,
            candidate_accepts: 0,
            candidate_rejections: 0,
            ace_count: 0,
            allow_ace_count: 0,
            deny_ace_count: 0,
            inherited_ace_count: 0,
            inherit_only_ace_count: 0,
            rename_attempted: false,
            rename_completed: false,
            rename_error_category: "not_run",
            identity_check: IdentityCheck::NotRun.category(),
            cleanup_complete: true,
            objects: Vec::new(),
        }
    }

    fn observed(&mut self, role: &'static str, verdict: Verdict) {
        self.objects.push(ObjectReport {
            role,
            outcome: if verdict.rejection.is_some() {
                "candidate_rejected"
            } else {
                "candidate_accepted"
            },
            category: verdict.category(),
            owner_matches: verdict.owner_matches,
            owner_category: verdict.owner_category.category(),
            dacl_category: verdict.dacl_category(),
            ace_count: verdict.counts.aces,
            allow_ace_count: verdict.counts.allow,
            deny_ace_count: verdict.counts.deny,
            inherited_ace_count: verdict.counts.inherited,
            inherit_only_ace_count: verdict.counts.inherit_only,
        });
        self.objects_observed += 1;
        self.ace_count += verdict.counts.aces;
        self.allow_ace_count += verdict.counts.allow;
        self.deny_ace_count += verdict.counts.deny;
        self.inherited_ace_count += verdict.counts.inherited;
        self.inherit_only_ace_count += verdict.counts.inherit_only;
        if verdict.rejection.is_some() {
            self.candidate_rejections += 1;
            if self.outcome == "candidate_accepted" {
                self.outcome = "candidate_rejected";
                self.category = verdict.category();
            }
        } else {
            self.candidate_accepts += 1;
        }
    }

    fn error(&mut self, category: &'static str) {
        self.outcome = "error";
        self.category = category;
    }
}

struct Object {
    handle: HANDLE,
    identity: Option<Identity>,
    directory: bool,
    delete: bool,
}

impl Object {
    fn close(&mut self) -> Result<()> {
        if self.handle.is_null() {
            return Ok(());
        }
        let handle = std::mem::replace(&mut self.handle, null_mut());
        // SAFETY: this wrapper uniquely owns the successful API handle.
        if unsafe { CloseHandle(handle) } == 0 {
            Err("cleanup_failed")
        } else {
            Ok(())
        }
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn admit(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        Err("admission_timeout")
    } else {
        Ok(())
    }
}

fn wide(path: &Path) -> Result<Vec<u16>> {
    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) || value.len() > 32760 {
        return Err("root_environment");
    }
    value.push(0);
    Ok(value)
}

fn open(path: &Path, kind: ProbeOpenKind, deadline: Instant) -> Result<Object> {
    let path = wide(path)?;
    admit(deadline)?;
    // READ_CONTROL is explicit. No content write access or security attributes
    // are supplied. Only the generated rename-destination root shares WRITE,
    // allowing the rename API's destination-directory access. Ancestors and
    // the child directory still deny WRITE. Every directory denies DELETE
    // sharing; files allow DELETE for the one empty rename.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES | if kind.delete() { DELETE } else { 0 },
            FILE_SHARE_READ
                | if kind.share_write() {
                    FILE_SHARE_WRITE
                } else {
                    0
                }
                | if kind.share_delete() {
                    FILE_SHARE_DELETE
                } else {
                    0
                },
            null(),
            if kind.create() {
                CREATE_NEW
            } else {
                OPEN_EXISTING
            },
            FILE_FLAG_OPEN_REPARSE_POINT
                | if kind.directory() {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    FILE_ATTRIBUTE_NORMAL
                },
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err("object_open");
    }
    Ok(Object {
        handle,
        identity: None,
        directory: kind.directory(),
        delete: kind.delete(),
    })
}

fn metadata(handle: HANDLE, deadline: Option<Instant>) -> Result<(Identity, ObjectFacts)> {
    let mut basic = BY_HANDLE_FILE_INFORMATION::default();
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let mut id = FILE_ID_INFO::default();
    let mut flags = 0;
    // SAFETY: each API receives a live owned handle and a correctly sized,
    // aligned, writable output. No pathname, volume name or account is queried.
    let check = || {
        if let Some(deadline) = deadline {
            admit(deadline)
        } else {
            Ok(())
        }
    };
    check()?;
    if unsafe { GetFileInformationByHandle(handle, &mut basic) } == 0 {
        return Err("object_metadata");
    }
    check()?;
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err("object_metadata");
    }
    check()?;
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err("object_metadata");
    }
    check()?;
    if unsafe {
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
        return Err("object_metadata");
    }
    if basic.dwFileAttributes != tag.FileAttributes {
        return Err("identity_changed");
    }
    check()?;
    let facts = ObjectFacts {
        disk: unsafe { GetFileType(handle) } == FILE_TYPE_DISK,
        directory: tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        reparse: tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || tag.ReparseTag != 0,
        links: basic.nNumberOfLinks,
        persistent_acls: flags & FILE_PERSISTENT_ACLS != 0,
        empty: basic.nFileSizeHigh == 0 && basic.nFileSizeLow == 0,
    };
    Ok((
        Identity {
            volume: id.VolumeSerialNumber,
            file: id.FileId.Identifier,
        },
        facts,
    ))
}

fn structurally_safe(facts: ObjectFacts, directory: bool) -> bool {
    facts.disk
        && facts.directory == directory
        && !facts.reparse
        && facts.persistent_acls
        && (directory || (facts.links == 1 && facts.empty))
}

fn effective_user(deadline: Instant) -> Result<Vec<u8>> {
    admit(deadline)?;
    let mut handle = null_mut();
    // OpenAsSelf changes the access-check context, never which token is read.
    // Process fallback is permitted ONLY for ERROR_NO_TOKEN.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut handle) } == 0 {
        if unsafe { GetLastError() } != ERROR_NO_TOKEN {
            return Err("token_open");
        }
        admit(deadline)?;
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
            return Err("token_open");
        }
    }
    let mut token = Object {
        handle,
        identity: None,
        directory: false,
        delete: false,
    };
    let result = (|| {
        let mut needed = 0;
        admit(deadline)?;
        if unsafe { GetTokenInformation(handle, TokenUser, null_mut(), 0, &mut needed) } != 0
            || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
        {
            return Err("token_query");
        }
        if needed < size_of::<TOKEN_USER>() as u32 || needed > 4096 {
            return Err("token_bounds");
        }
        // usize storage guarantees TOKEN_USER alignment. Keep all pointer reads
        // inside the returned, bounded allocation before parsing SID bytes.
        let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        admit(deadline)?;
        let capacity = needed;
        if unsafe {
            GetTokenInformation(
                handle,
                TokenUser,
                storage.as_mut_ptr().cast(),
                capacity,
                &mut needed,
            )
        } == 0
        {
            return Err("token_query");
        }
        if needed > capacity || needed < size_of::<TOKEN_USER>() as u32 {
            return Err("token_bounds");
        }
        let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
        let start = storage.as_ptr() as usize;
        let sid_at = (user.User.Sid as usize)
            .checked_sub(start)
            .ok_or("token_bounds")?;
        if sid_at < size_of::<TOKEN_USER>() || sid_at >= needed as usize {
            return Err("token_bounds");
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), needed as usize) };
        Ok(policy::sid_prefix(&bytes[sid_at..])
            .map_err(|_| "token_bounds")?
            .to_vec())
    })();
    token.close()?;
    result
}

fn descriptor(
    handle: HANDLE,
    user: &[u8],
    facts: ObjectFacts,
    directory: bool,
    deadline: Instant,
    report: &mut RootReport,
) -> Result<Verdict> {
    admit(deadline)?;
    let information = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut needed = 0;
    // Caller-sized handle query: reject before allocating over 64 KiB, and do
    // not retry if the descriptor grows between the size and data calls.
    if unsafe { GetKernelObjectSecurity(handle, information, null_mut(), 0, &mut needed) } != 0
        || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
    {
        return Err("descriptor_read");
    }
    if needed as usize > MAX_DESCRIPTOR_BYTES {
        return Err("descriptor_limit");
    }
    if needed < 20 {
        return Err("malformed_descriptor");
    }
    let capacity = needed;
    let mut storage = vec![0u32; (capacity as usize).div_ceil(size_of::<u32>())];
    admit(deadline)?;
    if unsafe {
        GetKernelObjectSecurity(
            handle,
            information,
            storage.as_mut_ptr().cast(),
            capacity,
            &mut needed,
        )
    } == 0
    {
        return Err("descriptor_read");
    }
    if needed > capacity || needed < 20 {
        return Err("malformed_descriptor");
    }
    report.descriptor_reads += 1;
    // SAFETY: only the bounded returned bytes of our aligned allocation are
    // exposed. The pure parser checks every offset, SID and ACE before use.
    let bytes =
        unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), needed as usize) };
    policy::assess(bytes, user, facts, directory).map_err(policy::ParseError::category)
}

fn verify(object: &Object, deadline: Instant) -> Result<()> {
    let (identity, facts) = metadata(object.handle, Some(deadline))?;
    if object.identity != Some(identity) {
        return Err("identity_changed");
    }
    if !structurally_safe(facts, object.directory) {
        return Err("unsafe_object");
    }
    Ok(())
}

fn pin_ancestors(base: &Path, objects: &mut Vec<Object>, deadline: Instant) -> Result<()> {
    // This checkpoint accepts only ordinary absolute drive paths, up to 32
    // components. UNC, device/verbatim paths and parent traversal are excluded.
    let components: Vec<_> = base.components().collect();
    if components.len() > 32
        || !matches!(components.first(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
        || !matches!(components.get(1), Some(Component::RootDir))
    {
        return Err("root_environment");
    }
    let mut path = PathBuf::from(components[0].as_os_str());
    path.push(components[1].as_os_str());
    let drive = wide(&path)?;
    admit(deadline)?;
    // Reject mapped network, removable, unknown and other drive classes before
    // opening any filesystem root. This is not host-wide network isolation.
    if unsafe { GetDriveTypeW(drive.as_ptr()) } != DRIVE_FIXED {
        return Err("unsafe_object");
    }
    for (index, component) in components.iter().enumerate().skip(1) {
        if index > 1 {
            if !matches!(component, Component::Normal(_)) {
                return Err("root_environment");
            }
            path.push(component.as_os_str());
        }
        objects.push(open(&path, ProbeOpenKind::Ancestor, deadline)?);
        let object = objects.last_mut().ok_or("object_open")?;
        let (identity, facts) = metadata(object.handle, Some(deadline))?;
        object.identity = Some(identity);
        if !structurally_safe(facts, true) {
            return Err("unsafe_object");
        }
    }
    Ok(())
}

fn verify_chain(objects: &[Object], deadline: Instant) -> Result<()> {
    for object in objects {
        verify(object, deadline)?;
    }
    Ok(())
}

fn observe(
    object: &mut Object,
    role: &'static str,
    user: &[u8],
    deadline: Instant,
    report: &mut RootReport,
) -> Result<()> {
    admit(deadline)?;
    let (identity, facts) = metadata(object.handle, Some(deadline))?;
    if let Some(expected) = object.identity {
        // Only the reopened renamed alias enters observe with an expected
        // identity. Record the actual comparison before descriptor inspection,
        // so a later inspection error cannot turn stable into not_run.
        let check = IdentityCheck::compare(expected, identity);
        report.identity_check = check.category();
        if check == IdentityCheck::Changed {
            return Err("identity_changed");
        }
    }
    object.identity = Some(identity);
    let verdict = descriptor(
        object.handle,
        user,
        facts,
        object.directory,
        deadline,
        report,
    )?;
    report.observed(role, verdict);
    if !structurally_safe(facts, object.directory) {
        return Err("unsafe_object");
    }
    // Candidate ownership/DACL rejection does not prevent additional EMPTY
    // observations. It must never be interpreted as permission to store data.
    Ok(())
}

fn rename_empty(
    handle: HANDLE,
    target: &Path,
    deadline: Instant,
    report: &mut RootReport,
) -> Result<()> {
    let name = wide(target)?;
    let bytes = offset_of!(FILE_RENAME_INFO, FileName) + name.len() * 2;
    let size = bytes.max(size_of::<FILE_RENAME_INFO>());
    let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: aligned zeroed storage is at least the struct plus full UTF-16
    // name. ReplaceIfExists stays FALSE and RootDirectory stays NULL.
    unsafe {
        (*info).FileNameLength = ((name.len() - 1) * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            name.len(),
        );
    }
    admit(deadline)?;
    report.rename_attempted = true;
    if unsafe { SetFileInformationByHandle(handle, FileRenameInfo, info.cast(), size as u32) } == 0
    {
        // Capture immediately, before calling the mapper or changing the
        // receipt. Only the fixed category leaves this function.
        let error = unsafe { GetLastError() };
        let category = policy::rename_error_category(error);
        report.rename_error_category = category;
        return Err(category);
    }
    report.rename_error_category = "none";
    report.rename_completed = true;
    Ok(())
}

fn cleanup(objects: &mut [Object]) -> bool {
    let mut good = true;
    for object in objects.iter_mut().rev() {
        if object.delete {
            // Never enumerate or recursively delete unknown children. Delete
            // only our original handle identity after another structural check.
            let verified = metadata(object.handle, None).is_ok_and(|(identity, facts)| {
                object.identity == Some(identity) && structurally_safe(facts, object.directory)
            });
            if verified {
                let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
                if unsafe {
                    SetFileInformationByHandle(
                        object.handle,
                        FileDispositionInfo,
                        (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                        size_of::<FILE_DISPOSITION_INFO>() as u32,
                    )
                } == 0
                {
                    good = false;
                }
            } else {
                good = false;
            }
        }
        if object.close().is_err() {
            good = false;
        }
    }
    good
}

fn disappeared(root: &Path, deadline: Instant) -> bool {
    let Ok(path) = wide(root) else {
        return false;
    };
    if admit(deadline).is_err() {
        return false;
    }
    // One observation only, while original ancestor handles remain pinned.
    // Delete-pending/access-denied, a replacement, or any other uncertainty is
    // not successful cleanup. Never retry or delete a newly discovered object.
    if unsafe { GetFileAttributesW(path.as_ptr()) } != INVALID_FILE_ATTRIBUTES {
        return false;
    }
    matches!(
        unsafe { GetLastError() },
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND
    )
}

fn root_case(label: &'static str, variable: &str, user: &[u8], deadline: Instant) -> RootReport {
    let mut report = RootReport::new(label);
    let mut objects = Vec::new();
    let mut generated_root = None;
    let mut captured_root = false;
    let mut ancestor_count = 0;
    let result = (|| {
        admit(deadline)?;
        let base = PathBuf::from(std::env::var_os(variable).ok_or("root_environment")?);
        if !base.is_absolute() {
            return Err("root_environment");
        }
        pin_ancestors(&base, &mut objects, deadline)?;
        ancestor_count = objects.len();
        verify_chain(&objects, deadline)?;
        admit(deadline)?;
        // keep() intentionally disables TempDir's recursive Drop. All deletion
        // below is by captured handles; an uncertain root is left as evidence.
        let root = tempfile::Builder::new()
            .prefix("cedar-recovery-descriptor-")
            .tempdir_in(&base)
            .map_err(|_| "root_create")?
            .keep();
        generated_root = Some(root.clone());
        report.cleanup_complete = false;
        report.stage = "root";
        let root_index = objects.len();
        objects.push(open(&root, ProbeOpenKind::RenameDestinationRoot, deadline)?);
        captured_root = true;
        observe(
            &mut objects[root_index],
            "root",
            user,
            deadline,
            &mut report,
        )?;
        report.stage = "child_directory";
        verify_chain(&objects, deadline)?;
        let child = root.join("probe.directory");
        admit(deadline)?;
        // std uses the ordinary inherited descriptor; no explicit security
        // descriptor or ACL is supplied for this additional empty directory.
        std::fs::create_dir(&child).map_err(|_| "root_create")?;
        objects.push(open(&child, ProbeOpenKind::ChildDirectory, deadline)?);
        observe(
            &mut objects[root_index + 1],
            "child_directory",
            user,
            deadline,
            &mut report,
        )?;
        report.stage = "lock";
        verify_chain(&objects, deadline)?;
        objects.push(open(
            &root.join("probe.lock"),
            ProbeOpenKind::EmptyFile,
            deadline,
        )?);
        observe(
            &mut objects[root_index + 2],
            "lock",
            user,
            deadline,
            &mut report,
        )?;
        report.stage = "temporary";
        verify_chain(&objects, deadline)?;
        objects.push(open(
            &root.join("probe.tmp"),
            ProbeOpenKind::EmptyFile,
            deadline,
        )?);
        observe(
            &mut objects[root_index + 3],
            "temporary",
            user,
            deadline,
            &mut report,
        )?;
        report.stage = "rename";
        verify_chain(&objects, deadline)?;
        let target = root.join("probe.renamed");
        rename_empty(
            objects[root_index + 3].handle,
            &target,
            deadline,
            &mut report,
        )?;
        report.stage = "renamed";
        verify_chain(&objects, deadline)?;
        objects.push(open(&target, ProbeOpenKind::RenamedAlias, deadline)?);
        // Reopened aliases do not own deletion; only the CREATE_NEW handle does.
        objects[root_index + 4].identity = objects[root_index + 3].identity;
        observe(
            &mut objects[root_index + 4],
            "renamed_record",
            user,
            deadline,
            &mut report,
        )?;
        report.stage = "complete";
        Ok(())
    })();
    if let Err(error) = result {
        report.error(error);
    }
    let (ancestors, generated) = objects.split_at_mut(ancestor_count);
    let generated_closed = cleanup(generated);
    let root_removed = generated_root.as_ref().is_none_or(|root| {
        captured_root
            && generated_closed
            && verify_chain(ancestors, deadline).is_ok()
            && disappeared(root, deadline)
    });
    let ancestors_closed = cleanup(ancestors);
    // A created root that could not be captured or observed absent must never
    // look cleaned up, even if deletion was acknowledged for another object.
    report.cleanup_complete = generated_closed && root_removed && ancestors_closed;
    if !report.cleanup_complete {
        report.stage = "cleanup";
        report.error("cleanup_failed");
    }
    report
}

pub fn run() -> Receipt {
    let deadline = Instant::now() + Duration::from_secs(55);
    let roots = match effective_user(deadline) {
        Ok(user) => [
            root_case("runner_temp", "RUNNER_TEMP", &user, deadline),
            root_case("local_app_data", "LOCALAPPDATA", &user, deadline),
        ],
        Err(error) => ["runner_temp", "local_app_data"].map(|label| {
            let mut report = RootReport::new(label);
            report.error(error);
            report
        }),
    };
    Receipt {
        schema_version: 2,
        probe: "windows_default_inherited_descriptor_probe",
        descriptor_query: "GetKernelObjectSecurity",
        shipping_unchanged: true,
        metadata_bytes_written: 0,
        body_bytes_written: 0,
        roots,
    }
}
