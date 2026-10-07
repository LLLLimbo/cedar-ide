//! Atomic, suspended process creation and handle-owned job lifetime.
//!
//! JOB_LIST requires Windows 10 / Server 2016. Unsupported attributes or an
//! incompatible enclosing job fail closed; there is no breakaway or unconfined
//! retry. HANDLE_LIST limits this child's inheritance only. The surrounding host
//! must control all spawning while the borrowed stdio handles are inheritable.
//!
//! Contracts checked against Microsoft's UpdateProcThreadAttribute,
//! InitializeProcThreadAttributeList, CreateProcessW, ResumeThread, HeapAlloc,
//! and Nested Jobs documentation. In particular, attribute *values* must remain
//! valid until DeleteProcThreadAttributeList, not merely until CreateProcessW.

use super::handles::{adopt, raw, ChildStdio};
use crate::command_line::{encode_command_line, validate_executable_text};
use crate::{LaunchSpec, ProcessExit};
use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::{Component, Path, Prefix};
use std::ptr::{null, null_mut, NonNull};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    DuplicateHandle, GetHandleInformation, ERROR_INSUFFICIENT_BUFFER, ERROR_PROCESS_ABORTED,
    HANDLE, HANDLE_FLAG_INHERIT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, IsProcessInJob, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapAlloc, HeapFree};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT,
    INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// Owns the root's handles and its private job, never the borrowed stdio handles.
/// Exit reporting is strictly a native fact; callers decide task outcomes.
pub(crate) struct ProcessOwner {
    // On an exceptional termination API failure, field drop closes the job
    // (KILL_ON_JOB_CLOSE) before RootHandles performs its final root wait.
    job: OwnedHandle,
    child: RootHandles,
    // Creation-time diagnostic identity only. Every operation uses owned
    // handles, never PID lookup; Windows can eventually recycle this number.
    process_id: u32,
    resumed: bool,
    termination_requested: bool,
    exit: Option<ProcessExit>,
}

impl ProcessOwner {
    pub(crate) fn create_suspended(spec: &LaunchSpec, stdio: &ChildStdio) -> io::Result<Self> {
        Self::create_checked(spec, stdio, |_| Ok(()))
    }

    // The private hook provides a failure/unwind seam after handle ownership but
    // before membership validation. Production supplies only the no-op above.
    fn create_checked(
        spec: &LaunchSpec,
        stdio: &ChildStdio,
        before_validation: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<Self> {
        let (application, cwd, mut command_line) = validated_launch(spec)?;
        check_stdio_inheritance(stdio)?;
        let job = create_job()?;
        let attributes = AttributeList::new(&job, stdio)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = raw(&stdio.stdin);
        startup.StartupInfo.hStdOutput = raw(&stdio.stdout);
        startup.StartupInfo.hStdError = raw(&stdio.stderr);
        startup.lpAttributeList = attributes.as_ptr();
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: application/cwd are absolute UTF-16 NUL-terminated buffers;
        // command_line is mutable, bounded, and NUL-terminated. STARTUPINFOEXW's
        // prefix and cb match EXTENDED_STARTUPINFO_PRESENT. Both attribute value
        // arrays, their job/stdio handles, and the aligned list remain live.
        // TRUE is required for HANDLE_LIST; null security attributes make the
        // returned process/thread handles non-inheritable. No breakaway flag.
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                // This is a non-interactive stdio task. Console executables
                // must not allocate/inherit a visible console behind the IDE.
                // GUI executables can still show their own application UI.
                // https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
                null(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut info,
            )
        };
        if created == 0 {
            // Capture before attributes/job destructors can change last-error.
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful CreateProcessW returns two distinct, newly owned,
        // valid non-pseudo handles. Wrap both immediately, with no fallible
        // operation or intervening Win32 call. The guard terminates and waits
        // the root even if later membership validation or unwinding fails.
        let child = unsafe {
            RootHandles {
                process: OwnedHandle::from_raw_handle(info.hProcess),
                thread: OwnedHandle::from_raw_handle(info.hThread),
            }
        };
        // AttributeList borrows job/stdio. Its destructor deletes the list
        // before freeing any attribute values; only then may job move.
        drop(attributes);
        let mut owner = Self {
            job,
            child,
            process_id: info.dwProcessId,
            resumed: false,
            termination_requested: false,
            exit: None,
        };
        before_validation(&mut owner)?;
        owner.verify_membership()?;
        Ok(owner)
    }

    pub(crate) fn process_id(&self) -> u32 {
        self.process_id
    }

    /// A non-inheritable duplicate for independently observing this exact root.
    /// The granted rights permit waiting/querying, never termination or mutation.
    /// It keeps the process object observable without keeping its job alive.
    pub(crate) fn observation_handle(&self) -> io::Result<OwnedHandle> {
        let mut duplicate = null_mut();
        // SAFETY: this borrowed pseudo handle denotes the current process and
        // is used only to identify the source/target handle tables, never owned.
        let current = unsafe { GetCurrentProcess() };
        // SAFETY: the source process handle remains owned throughout this call,
        // both tables are this process's, and duplicate is a writable HANDLE.
        // Request only SYNCHRONIZE plus limited query access. FALSE and options
        // zero prohibit inheritance, SAME_ACCESS, and closing the source.
        if unsafe {
            DuplicateHandle(
                current,
                raw(&self.child.process),
                current,
                &mut duplicate,
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                0,
            )
        } == 0
        {
            // Capture failure before any cleanup or other Win32 call.
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a successful DuplicateHandle returned a newly owned actual
        // process handle in this process. Adopt without an intervening OS call.
        unsafe { adopt(duplicate) }
    }

    fn verify_membership(&self) -> io::Result<()> {
        let mut member = 0;
        // SAFETY: both handles remain owned and valid for the complete call;
        // member is a writable BOOL-sized output.
        if unsafe { IsProcessInJob(raw(&self.child.process), raw(&self.job), &mut member) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if member == 0 {
            return Err(io::Error::other(
                "created process was not assigned to its required job",
            ));
        }
        Ok(())
    }

    /// Resume exactly once, only after atomic assignment and validation.
    pub(crate) fn resume(&mut self) -> io::Result<()> {
        if self.resumed || self.termination_requested {
            return Err(invalid_input(
                "process is no longer awaiting its initial resume",
            ));
        }
        // SAFETY: CreateProcessW returned this live, owned thread handle with
        // THREAD_SUSPEND_RESUME. This is the sole production resume operation.
        let previous_count = unsafe { ResumeThread(raw(&self.child.thread)) };
        let error = match previous_count {
            1 => {
                self.resumed = true;
                return Ok(());
            }
            u32::MAX => io::Error::last_os_error(),
            _ => io::Error::other("initial thread suspend count was not exactly one"),
        };
        // Save the ResumeThread error first. Even count=0 (already runnable) or
        // count>1 (still suspended) must not leave a child behind after failure.
        let job_terminated = self.terminate_tree().is_ok();
        let _ = self.child.terminate_and_wait();
        if job_terminated {
            let _ = self.wait_job_empty();
        }
        Err(error)
    }

    pub(crate) fn try_exit(&mut self) -> io::Result<Option<ProcessExit>> {
        if let Some(exit) = self.exit {
            return Ok(Some(exit));
        }
        if !process_signaled(&self.child.process, 0)? {
            return Ok(None);
        }
        self.record_signaled_exit().map(Some)
    }

    /// Request tree termination; completion is observed by wait_exit/accounting.
    /// The chosen native termination code does not classify the task outcome.
    pub(crate) fn terminate_tree(&mut self) -> io::Result<()> {
        self.termination_requested = true;
        // SAFETY: this private, owned job was created with all-access rights.
        // Nested jobs and descendants are included by TerminateJobObject.
        if unsafe { TerminateJobObject(raw(&self.job), ERROR_PROCESS_ABORTED) } == 0 {
            let error = io::Error::last_os_error();
            // Retain the original job error; independently stop the owned root
            // even if job termination itself unexpectedly fails.
            let _ = self.child.terminate_and_wait();
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn wait_exit(&mut self) -> io::Result<ProcessExit> {
        if let Some(exit) = self.exit {
            return Ok(exit);
        }
        if !process_signaled(&self.child.process, INFINITE)? {
            return Err(io::Error::other(
                "infinite process wait unexpectedly timed out",
            ));
        }
        self.record_signaled_exit()
    }

    fn record_signaled_exit(&mut self) -> io::Result<ProcessExit> {
        let mut code = 0;
        // SAFETY: callers first observed this owned process handle signaled;
        // CreateProcessW grants the required query right and code is writable.
        if unsafe { GetExitCodeProcess(raw(&self.child.process), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // 259 is a legitimate *exited* code. Never use it as a liveness test.
        let exit = ProcessExit { code };
        self.exit = Some(exit);
        Ok(exit)
    }

    pub(crate) fn active_processes(&self) -> io::Result<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the class, output structure, and length agree. This live job
        // has JOB_OBJECT_QUERY and no pointers escape the synchronous call.
        if unsafe {
            QueryInformationJobObject(
                raw(&self.job),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(accounting.ActiveProcesses)
    }

    fn wait_job_empty(&self) -> io::Result<()> {
        // A job is not a general-purpose wait-for-empty event. Poll accounting
        // after termination rather than interpreting a job handle's signal or
        // enumerating/reopening PIDs. No breakaway is allowed in this job.
        while self.active_processes()? != 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
}

impl Drop for ProcessOwner {
    fn drop(&mut self) {
        let job_terminated = self.terminate_tree().is_ok();
        // Essential even if membership verification failed: an unexpectedly
        // unassigned suspended process is outside this job's kill coverage.
        let _ = self.child.terminate_and_wait();
        if job_terminated {
            let _ = self.wait_job_empty();
        }
        // Destructors cannot report OS failures. The job's final close remains
        // a kill-on-close backstop; explicit methods report their Win32 errors.
    }
}

/// Guards the interval between successful creation and a validated owner too.
struct RootHandles {
    process: OwnedHandle,
    thread: OwnedHandle,
}

impl RootHandles {
    fn terminate_and_wait(&self) -> io::Result<()> {
        if matches!(process_signaled(&self.process, 0), Ok(true)) {
            return Ok(());
        }
        // SAFETY: this is the actual newly-created process HANDLE, never a PID
        // or pseudo handle, and retains PROCESS_TERMINATE/SYNCHRONIZE rights.
        let termination_error =
            if unsafe { TerminateProcess(raw(&self.process), ERROR_PROCESS_ABORTED) } == 0 {
                Some(io::Error::last_os_error())
            } else {
                None
            };
        // TerminateProcess is asynchronous. Waiting also handles its race with
        // an already-exiting root (where termination can return access denied).
        match process_signaled(&self.process, INFINITE) {
            Ok(true) => Ok(()),
            Ok(false) => Err(termination_error.unwrap_or_else(|| {
                io::Error::other("infinite process wait unexpectedly timed out")
            })),
            Err(error) => Err(termination_error.unwrap_or(error)),
        }
    }
}

impl Drop for RootHandles {
    fn drop(&mut self) {
        let _ = self.terminate_and_wait();
    }
}

fn process_signaled(process: &OwnedHandle, timeout: u32) -> io::Result<bool> {
    // SAFETY: the borrowed process handle remains live for this wait and was
    // created with SYNCHRONIZE. No concurrent code closes owned handles.
    match unsafe { WaitForSingleObject(raw(process), timeout) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other("unexpected process wait result")),
    }
}

fn create_job() -> io::Result<OwnedHandle> {
    // SAFETY: null name creates a fresh unnamed job, and null attributes select
    // a non-inheritable handle. Adopt before another call can change last-error.
    let job = unsafe { adopt(CreateJobObjectW(null(), null()))? };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // No breakaway, silent-breakaway, or UI restrictions. This is compatible
    // with legal enclosing/nested job configurations; incompatibilities fail.
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: limits exactly matches the requested information class/size,
    // remains live for the call, and this owned job has JOB_OBJECT_SET_ATTRIBUTES.
    if unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

fn check_stdio_inheritance(stdio: &ChildStdio) -> io::Result<()> {
    for handle in stdio.raw_handles() {
        let mut flags = 0;
        // SAFETY: ChildStdio owns each non-pseudo handle for this call and for
        // the entire launch. GetHandleInformation writes only this u32.
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if flags & HANDLE_FLAG_INHERIT == 0 {
            return Err(invalid_input("child stdio handle is not inheritable"));
        }
    }
    Ok(())
}

/// Own both the aligned opaque storage and heap-stable referenced value arrays.
struct AttributeList<'a> {
    storage: NonNull<c_void>,
    heap: HANDLE,
    initialized: bool,
    inherited: Box<[HANDLE; 3]>,
    jobs: Box<[HANDLE; 1]>,
    // The arrays hold borrowed values, not additional ownership. These borrows
    // keep all referred-to handles valid until this list has been deleted.
    _job: &'a OwnedHandle,
    _stdio: &'a ChildStdio,
}

impl<'a> AttributeList<'a> {
    fn new(job: &'a OwnedHandle, stdio: &'a ChildStdio) -> io::Result<Self> {
        let inherited = Box::new(stdio.raw_handles());
        let jobs = Box::new([raw(job)]);
        let mut bytes = 0;
        // SAFETY: the documented sizing call uses a null list and writable
        // SIZE_T. Two entries are required and reserved flags are zero.
        let probe = unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes) };
        let probe_error = io::Error::last_os_error();
        if probe != 0 || bytes == 0 {
            return Err(io::Error::other("unexpected attribute-list sizing result"));
        }
        if probe_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return Err(probe_error);
        }
        // SAFETY: GetProcessHeap returns a borrowed heap, never to be closed.
        let heap = unsafe { GetProcessHeap() };
        if heap.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: allocate the exact queried size on the serialized process
        // heap. HeapAlloc provides Windows MEMORY_ALLOCATION_ALIGNMENT (16/8),
        // as in Microsoft's attribute-list example; Vec<u8> would not promise
        // this alignment. HeapAlloc does NOT set last-error on allocation failure.
        let storage = NonNull::new(unsafe { HeapAlloc(heap, 0, bytes) })
            .ok_or_else(|| io::Error::from(io::ErrorKind::OutOfMemory))?;
        let mut list = Self {
            storage,
            heap,
            initialized: false,
            inherited,
            jobs,
            _job: job,
            _stdio: stdio,
        };
        // SAFETY: storage has the exact queried size and required alignment;
        // reserved flags/count match the sizing call. It is owned by list.
        if unsafe { InitializeProcThreadAttributeList(list.as_ptr(), 2, 0, &mut bytes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        list.initialized = true;
        // SAFETY: both boxed arrays have fixed addresses even if list moves.
        // They and the borrowed handles outlive DeleteProcThreadAttributeList.
        // HANDLE_LIST is exactly the three child standard handles; notably it
        // excludes the job, parent pipe ends, process/thread handles and events.
        if unsafe {
            UpdateProcThreadAttribute(
                list.as_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                list.inherited.as_ptr().cast(),
                size_of::<[HANDLE; 3]>(),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the private job handle is in stable boxed storage until list
        // deletion. JOB_LIST assigns atomically with process creation. Any
        // unsupported-attribute/nesting error is returned without a fallback.
        if unsafe {
            UpdateProcThreadAttribute(
                list.as_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                list.jobs.as_ptr().cast(),
                size_of::<[HANDLE; 1]>(),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(list)
    }

    fn as_ptr(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_ptr()
    }
}

impl Drop for AttributeList<'_> {
    fn drop(&mut self) {
        // SAFETY: only successfully initialized lists may be deleted. Rust runs
        // this destructor body before dropping the two Box fields, so referenced
        // attribute value memory is still live during deletion. The unique
        // buffer is then released to the same process heap that allocated it.
        unsafe {
            if self.initialized {
                DeleteProcThreadAttributeList(self.as_ptr());
            }
            HeapFree(self.heap, 0, self.storage.as_ptr());
        }
    }
}

fn validated_launch(spec: &LaunchSpec) -> io::Result<(Vec<u16>, Vec<u16>, Vec<u16>)> {
    let executable = absolute_utf8(&spec.executable)?;
    let cwd = absolute_utf8(&spec.cwd)?;
    validate_executable_text(executable)?;
    let command_line = encode_command_line(executable, &spec.arguments)?;
    if !std::fs::metadata(&spec.executable)?.is_file() {
        return Err(invalid_input("executable must be an ordinary .exe file"));
    }
    if !std::fs::metadata(&spec.cwd)?.is_dir() {
        return Err(invalid_input(
            "working directory must be an existing directory",
        ));
    }
    // These checks neither authorize the executable nor lock filesystem names;
    // the caller owns authorization and CreateProcessW handles filesystem races.
    Ok((
        executable.encode_utf16().chain([0]).collect(),
        cwd.encode_utf16().chain([0]).collect(),
        command_line,
    ))
}

fn absolute_utf8(path: &Path) -> io::Result<&str> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid_input("Windows launch paths must be valid UTF-8"))?;
    if !path.is_absolute() || text.contains('\0') {
        return Err(invalid_input(
            "Windows launch paths must be absolute and NUL-free",
        ));
    }
    match path.components().next() {
        Some(Component::Prefix(prefix))
            if matches!(
                prefix.kind(),
                Prefix::Disk(_)
                    | Prefix::VerbatimDisk(_)
                    | Prefix::UNC(_, _)
                    | Prefix::VerbatimUNC(_, _)
            ) => {}
        _ => return Err(invalid_input("Windows launch requires a filesystem path")),
    }
    Ok(text)
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use windows_sys::Win32::Foundation::{
        SetHandleInformation, ERROR_ACCESS_DENIED, GENERIC_READ, GENERIC_WRITE,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::JobObjects::{
        JobObjectBasicUIRestrictions, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    };
    use windows_sys::Win32::System::Threading::SuspendThread;

    fn null_stdio() -> ChildStdio {
        fn open(access: u32) -> OwnedHandle {
            let name: Vec<u16> = "NUL\0".encode_utf16().collect();
            let security = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: null_mut(),
                bInheritHandle: 1,
            };
            // SAFETY: a literal NUL device name and live SECURITY_ATTRIBUTES;
            // the newly opened inheritable handle is adopted exactly once.
            unsafe {
                adopt(CreateFileW(
                    name.as_ptr(),
                    access,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    &security,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    null_mut(),
                ))
                .unwrap()
            }
        }
        ChildStdio {
            stdin: open(GENERIC_READ),
            stdout: open(GENERIC_WRITE),
            stderr: open(GENERIC_WRITE),
        }
    }

    fn self_spec() -> LaunchSpec {
        LaunchSpec {
            executable: std::env::current_exe().unwrap(),
            // If a test accidentally resumes, only list tests. Never recurse
            // into the test suite and create an uncontrolled process storm.
            arguments: vec!["--list".into()],
            cwd: std::env::current_dir().unwrap(),
        }
    }

    fn assert_dead(process: &OwnedHandle) {
        assert!(process_signaled(process, 5_000).unwrap());
    }

    #[test]
    fn job_is_noninheritable_kill_on_close_only_without_ui_restrictions() {
        let job = create_job().unwrap();
        let mut flags = 0;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let mut ui = JOBOBJECT_BASIC_UI_RESTRICTIONS::default();
        // SAFETY: live owned job and correctly sized typed output buffers.
        unsafe {
            assert_ne!(GetHandleInformation(raw(&job), &mut flags), 0);
            assert_ne!(
                QueryInformationJobObject(
                    raw(&job),
                    JobObjectExtendedLimitInformation,
                    (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    null_mut(),
                ),
                0
            );
            assert_ne!(
                QueryInformationJobObject(
                    raw(&job),
                    JobObjectBasicUIRestrictions,
                    (&mut ui as *mut JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
                    size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
                    null_mut(),
                ),
                0
            );
        }
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        assert_eq!(
            limits.BasicLimitInformation.LimitFlags,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        );
        assert_eq!(ui.UIRestrictionsClass, 0);
    }

    #[test]
    fn attribute_values_and_aligned_storage_survive_owner_moves() {
        let job = create_job().unwrap();
        let stdio = null_stdio();
        let attributes = AttributeList::new(&job, &stdio).unwrap();
        let inherited_address = attributes.inherited.as_ptr();
        let jobs_address = attributes.jobs.as_ptr();
        let list_address = attributes.as_ptr();
        let minimum_alignment = if cfg!(target_pointer_width = "64") {
            16
        } else {
            8
        };
        assert_eq!(list_address as usize % minimum_alignment, 0);
        // Move the Rust owner to a different address; every Win32 pointer stays
        // valid because it refers to heap storage, never a movable stack array.
        let moved = Box::new(attributes);
        assert_eq!(moved.inherited.as_ptr(), inherited_address);
        assert_eq!(moved.jobs.as_ptr(), jobs_address);
        assert_eq!(moved.as_ptr(), list_address);
        assert_eq!(*moved.inherited, stdio.raw_handles());
        assert_eq!(*moved.jobs, [raw(&job)]);
        drop(moved);
        check_stdio_inheritance(&stdio).unwrap();
    }

    #[test]
    fn unresumed_creation_is_owned_confined_and_drop_joins_it() {
        let stdio = null_stdio();
        let mut owner = ProcessOwner::create_suspended(&self_spec(), &stdio).unwrap();
        let observer = owner.observation_handle().unwrap();
        assert_eq!(owner.try_exit().unwrap(), None);
        assert_eq!(owner.active_processes().unwrap(), 1);
        owner.verify_membership().unwrap();
        drop(owner);
        assert_dead(&observer);
        // The owner borrowed, and did not close or mutate, the child stdio.
        check_stdio_inheritance(&stdio).unwrap();
    }

    #[test]
    fn observation_handle_can_wait_and_query_but_not_terminate_or_inherit() {
        let stdio = null_stdio();
        let owner = ProcessOwner::create_suspended(&self_spec(), &stdio).unwrap();
        let observer = owner.observation_handle().unwrap();
        let mut flags = 0;
        // SAFETY: observer is a live owned process handle, flags is writable.
        assert_ne!(
            unsafe { GetHandleInformation(raw(&observer), &mut flags) },
            0
        );
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        assert!(!process_signaled(&observer, 0).unwrap());
        // SAFETY: this deliberate rights test targets only our suspended child.
        // A correctly restricted observation handle cannot terminate it.
        let terminated = unsafe { TerminateProcess(raw(&observer), 42) };
        let error = io::Error::last_os_error();
        assert_eq!(terminated, 0);
        assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
        assert!(!process_signaled(&observer, 0).unwrap());
        drop(owner);
        assert_dead(&observer);
        let mut code = 0;
        // SAFETY: the restricted handle still identifies the same, now-signaled
        // process and grants PROCESS_QUERY_LIMITED_INFORMATION; code is writable.
        assert_ne!(unsafe { GetExitCodeProcess(raw(&observer), &mut code) }, 0);
        assert_eq!(code, ERROR_PROCESS_ABORTED);
    }

    #[test]
    fn mismatched_job_validation_also_terminates_the_owned_process_handle() {
        let stdio = null_stdio();
        let mut observer = None;
        let mut actual_job = None;
        let result = ProcessOwner::create_checked(&self_spec(), &stdio, |owner| {
            observer = Some(owner.observation_handle()?);
            // Keep the real job open. The owner now checks/terminates an empty
            // different job, simulating an unexpectedly unassigned root without
            // ever creating an unconfined process or releasing its real job.
            actual_job = Some(std::mem::replace(&mut owner.job, create_job()?));
            Ok(())
        });
        assert!(result.is_err());
        assert_dead(observer.as_ref().unwrap());
        // If cleanup relied only on owner.job, the process would still be
        // suspended because this handle keeps its actual job alive.
        assert!(actual_job.is_some());
        drop(actual_job);
    }

    #[test]
    fn error_after_creation_preserves_error_and_joins_before_return() {
        let stdio = null_stdio();
        let mut observer = None;
        let result = ProcessOwner::create_checked(&self_spec(), &stdio, |owner| {
            observer = Some(owner.observation_handle()?);
            Err(io::Error::from_raw_os_error(1234))
        });
        assert_eq!(result.err().unwrap().raw_os_error(), Some(1234));
        assert_dead(observer.as_ref().unwrap());
    }

    #[test]
    fn unwind_after_creation_joins_before_leaving_scope() {
        let stdio = null_stdio();
        let mut observer = None;
        let caught = catch_unwind(AssertUnwindSafe(|| {
            let _ = ProcessOwner::create_checked(&self_spec(), &stdio, |owner| {
                observer = Some(owner.observation_handle()?);
                panic!("injected failure after taking process ownership");
            });
        }));
        assert!(caught.is_err());
        assert_dead(observer.as_ref().unwrap());
    }

    #[test]
    fn unexpected_suspend_count_fails_closed_and_joins() {
        let stdio = null_stdio();
        let mut owner = ProcessOwner::create_suspended(&self_spec(), &stdio).unwrap();
        // SAFETY: the thread is owned and suspended; this deliberate extra
        // suspension exercises the failure branch without executing the child.
        assert_eq!(unsafe { SuspendThread(raw(&owner.child.thread)) }, 1);
        assert!(owner.resume().is_err());
        assert_eq!(
            owner.try_exit().unwrap().unwrap().code,
            ERROR_PROCESS_ABORTED
        );
        assert_eq!(owner.active_processes().unwrap(), 0);
        assert!(owner.resume().is_err());
    }

    #[test]
    fn signaled_exit_preserves_259_and_all_u32_bits() {
        let stdio = null_stdio();
        for code in [259, 0x8000_0000, 0xC000_0005, u32::MAX] {
            let mut owner = ProcessOwner::create_suspended(&self_spec(), &stdio).unwrap();
            assert_eq!(owner.try_exit().unwrap(), None);
            // SAFETY: terminate the actual owned process HANDLE. The child need
            // never run to exercise every native exit-code bit and the 259 trap.
            assert_ne!(
                unsafe { TerminateProcess(raw(&owner.child.process), code) },
                0
            );
            assert_eq!(owner.wait_exit().unwrap(), ProcessExit { code });
            assert_eq!(owner.try_exit().unwrap(), Some(ProcessExit { code }));
            assert_eq!(owner.wait_exit().unwrap(), ProcessExit { code });
        }
    }

    #[test]
    fn initial_resume_succeeds_once_and_cannot_be_repeated() {
        let stdio = null_stdio();
        let mut owner = ProcessOwner::create_suspended(&self_spec(), &stdio).unwrap();
        owner.resume().unwrap();
        assert!(owner.resume().is_err());
        // --list is finite, writes only to NUL, and runs no tests.
        assert_eq!(owner.wait_exit().unwrap().code, 0);
    }

    #[test]
    fn termination_before_resume_prevents_any_future_resume() {
        let stdio = null_stdio();
        let mut owner = ProcessOwner::create_suspended(&self_spec(), &stdio).unwrap();
        owner.terminate_tree().unwrap();
        assert!(owner.resume().is_err());
        assert_eq!(owner.wait_exit().unwrap().code, ERROR_PROCESS_ABORTED);
    }

    #[test]
    fn noninheritable_stdio_is_rejected_before_creation() {
        let stdio = null_stdio();
        // SAFETY: this test owns the handle and intentionally removes only its
        // inheritance flag; no other thread has access to this ChildStdio.
        assert_ne!(
            unsafe { SetHandleInformation(raw(&stdio.stdout), HANDLE_FLAG_INHERIT, 0) },
            0
        );
        let error = ProcessOwner::create_suspended(&self_spec(), &stdio)
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn creation_failure_leaves_borrowed_stdio_usable() {
        let temp = tempfile::tempdir().unwrap();
        let fake_executable = temp.path().join("not-a-native-binary.exe");
        std::fs::write(&fake_executable, b"not a Windows executable").unwrap();
        let stdio = null_stdio();
        let spec = LaunchSpec {
            executable: fake_executable,
            arguments: Vec::new(),
            cwd: temp.path().to_owned(),
        };
        let error = ProcessOwner::create_suspended(&spec, &stdio).err().unwrap();
        assert!(error.raw_os_error().is_some());
        check_stdio_inheritance(&stdio).unwrap();
    }

    #[test]
    fn path_policy_rejects_relative_device_nul_and_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        for path in [
            "tool.exe",
            "C:tool.exe",
            r"\tool.exe",
            r"\\.\device.exe",
            "C:\\a\0b.exe",
        ] {
            assert!(absolute_utf8(Path::new(path)).is_err(), "{path:?}");
        }
        let invalid_unicode =
            OsString::from_wide(&[b'C' as u16, b':' as u16, b'\\' as u16, 0xD800]);
        assert!(absolute_utf8(Path::new(&invalid_unicode)).is_err());
        for path in [
            r"C:\dir\tool.exe",
            r"\\?\C:\dir\tool.exe",
            r"\\server\share\tool.exe",
        ] {
            assert_eq!(absolute_utf8(Path::new(path)).unwrap(), path);
        }
        let mut spec = self_spec();
        spec.executable.set_extension("cmd");
        assert_eq!(
            validated_launch(&spec).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
