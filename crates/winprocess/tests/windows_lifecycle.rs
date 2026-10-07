//! Real Windows tests, deliberately opt-in and serial.
//!
//! Prebuild cedar-winprocess-fixture with `--features fixtures`, set the exact
//! absolute executable path in CEDAR_WINPROCESS_FIXTURE_BIN, then run this test
//! target with `--ignored --test-threads=1`. Cross-compilation is not execution.
//! No compiler discovery, shell, network, PID-based termination, or IDE task
//! capability is involved. Every fixture has a five-second internal cap.
#![cfg(windows)]

use cedar_winprocess::{
    CaptureProgress, LaunchSpec, ProcessExit, StdinCancelOutcome, StdinWriteProgress, Stream,
    WindowsCommand, MAX_STDIN_WRITE_BYTES,
};
use std::fs;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, GetExitCodeProcess, GetProcessHandleCount, GetProcessId,
    OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};

const WAIT: Duration = Duration::from_secs(3);
const LIFETIME_CAP_EXIT: u32 = 124;

// A final guard against a primitive blocking before the fixture can start its
// own watchdog (notably suspended Drop). OS close-on-exit cleans owned jobs.
// Use serial execution: exiting this controlled test process cannot strand a
// fixture beyond its own cap or race another test's open inheritable sentinel.
struct Watchdog {
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    fn start() -> Self {
        let (stop, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            if receiver.recv_timeout(Duration::from_secs(25)).is_err() {
                eprintln!("Windows lifecycle test exceeded its watchdog deadline");
                std::process::exit(126);
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn fixture() -> PathBuf {
    let path = PathBuf::from(std::env::var_os("CEDAR_WINPROCESS_FIXTURE_BIN").expect(
        "set CEDAR_WINPROCESS_FIXTURE_BIN to the prebuilt native fixture .exe; tests do not compile it",
    ));
    assert!(path.is_absolute(), "fixture path must be absolute");
    assert!(path.is_file(), "fixture does not exist: {}", path.display());
    assert!(path.to_str().is_some(), "fixture path must be UTF-8");
    assert_eq!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("exe"),
        "fixture must be a native .exe"
    );
    path
}

fn text_path(path: &Path) -> String {
    path.to_str().expect("test path must be UTF-8").to_owned()
}

fn spec(dir: &Path, arguments: Vec<String>) -> LaunchSpec {
    LaunchSpec {
        executable: fixture(),
        arguments,
        cwd: dir.to_owned(),
    }
}

fn launch(dir: &Path, arguments: Vec<String>) -> WindowsCommand {
    WindowsCommand::spawn_suspended(&spec(dir, arguments)).expect("spawn suspended fixture")
}

#[derive(Default)]
struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Output {
    fn round(&mut self, command: &mut WindowsCommand) -> CaptureProgress {
        let before = self.stdout.len() + self.stderr.len();
        let progress = command
            .capture_round(|stream, bytes| match stream {
                Stream::Stdout => self.stdout.extend_from_slice(bytes),
                Stream::Stderr => self.stderr.extend_from_slice(bytes),
            })
            .expect("capture round");
        assert_eq!(
            progress.bytes,
            self.stdout.len() + self.stderr.len() - before,
            "capture byte accounting"
        );
        progress
    }

    fn complete(&mut self, command: &mut WindowsCommand) -> ProcessExit {
        let deadline = Instant::now() + WAIT;
        let mut exit = None;
        loop {
            let progress = self.round(command);
            if exit.is_none() {
                if let Some(code) = command.try_exit().expect("observe root exit") {
                    exit = Some(code);
                    // Root exit is observation only. The owner explicitly
                    // kills descendants before its final bounded drain.
                    command.terminate_tree().expect("clean up root's job");
                }
            }
            if progress.stdout_eof && progress.stderr_eof {
                if let Some(exit) = exit {
                    assert_eq!(command.wait_exit().expect("join root"), exit);
                    return exit;
                }
            }
            assert!(Instant::now() < deadline, "fixture capture/exit timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn drain(&mut self, command: &mut WindowsCommand) {
        let deadline = Instant::now() + WAIT;
        loop {
            let progress = self.round(command);
            if progress.stdout_eof && progress.stderr_eof {
                return;
            }
            assert!(Instant::now() < deadline, "capture drain timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn wait_file(path: &Path) {
    let deadline = Instant::now() + WAIT;
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "fixture did not publish {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_empty_job(command: &WindowsCommand) {
    let deadline = Instant::now() + WAIT;
    while command.active_processes().expect("query job accounting") != 0 {
        assert!(Instant::now() < deadline, "job still has active processes");
        thread::sleep(Duration::from_millis(5));
    }
}

fn assert_job_accounts_for_live_fixtures(command: &WindowsCommand, fixtures: u32) {
    // CREATE_NO_WINDOW suppresses a console window, not necessarily the OS's
    // console-server process. Job accounting may include that helper in
    // addition to our known, independently observed live fixtures. Do not tie
    // ownership to an undocumented exact helper count. Cleanup still requires
    // every observed fixture to terminate AND the whole job to reach zero.
    // https://github.com/microsoft/terminal/blob/main/doc/specs/%23492%20-%20Default%20Terminal/spec.md
    let active = command.active_processes().unwrap();
    assert!(
        active >= fixtures,
        "job accounts for {active} active processes but {fixtures} fixtures are known live"
    );
}

/// An observation-only handle to this fixture's already-running process.
/// Never use the published PID to terminate or modify a process. Opening while
/// it is alive gives a stable waitable object even after the PID is recycled.
struct ObservedProcess(OwnedHandle);

impl ObservedProcess {
    fn from_file(path: &Path) -> Self {
        wait_file(path);
        let pid = fs::read_to_string(path)
            .expect("read fixture pid")
            .parse::<u32>()
            .expect("parse fixture pid");
        // SAFETY: PID comes from our controlled fixture in our private temp
        // directory. No pointer arguments. Only wait/query access is requested,
        // and FALSE prevents inheritance. The returned owned handle is closed
        // exactly once by Drop. Microsoft OpenProcess and process-access docs:
        // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess
        // https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        assert!(
            !handle.is_null(),
            "OpenProcess: {}",
            io::Error::last_os_error()
        );
        // SAFETY: OpenProcess returned a fresh owned handle. Ownership is
        // transferred exactly once to OwnedHandle for automatic CloseHandle.
        let observed = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        assert!(observed.alive(), "fixture was not alive when observed");
        observed
    }

    fn alive(&self) -> bool {
        // SAFETY: This owned live handle has SYNCHRONIZE access and remains
        // open throughout a zero-timeout, non-mutating process wait.
        // https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject
        match unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => true,
            WAIT_OBJECT_0 => false,
            result => panic!("process observation failed: {result:#x}"),
        }
    }

    fn assert_terminated(&self) {
        // SAFETY: Same owned SYNCHRONIZE handle, held open for the whole wait.
        let result = unsafe { WaitForSingleObject(self.0.as_raw_handle(), 1500) };
        assert_eq!(result, WAIT_OBJECT_0, "owned fixture was not terminated");
        let mut code = 0;
        // SAFETY: Handle has QUERY_LIMITED_INFORMATION and code points to a
        // writable DWORD. We first proved termination using its wait handle,
        // rather than confusing the valid exit code 259 with STILL_ACTIVE.
        // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getexitcodeprocess
        assert_ne!(
            unsafe { GetExitCodeProcess(self.0.as_raw_handle(), &mut code) },
            0
        );
        assert_ne!(code, LIFETIME_CAP_EXIT, "fixture reached its safety cap");
    }
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn assigns_job_before_resume_and_never_executes_while_suspended() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let marker = dir.path().join("ran");
    let mut command = launch(dir.path(), vec!["marker".into(), text_path(&marker)]);
    assert_eq!(command.active_processes().unwrap(), 1);
    assert_eq!(command.try_exit().unwrap(), None);
    thread::sleep(Duration::from_millis(100));
    assert!(!marker.exists(), "child ran before explicit resume");
    command.resume().unwrap();
    let mut output = Output::default();
    assert_eq!(output.complete(&mut command).code, 0);
    assert_eq!(fs::read(&marker).unwrap(), b"ran");
    assert_eq!(output.stdout, b"marker-ran\n");
    wait_empty_job(&command);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn dropping_suspended_command_never_runs_user_code() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let marker = dir.path().join("must-not-run");
    let command = launch(dir.path(), vec!["marker".into(), text_path(&marker)]);
    assert_eq!(command.active_processes().unwrap(), 1);
    let root = ObservedProcess(command.observation_handle().unwrap());
    assert!(root.alive());
    let start = Instant::now();
    drop(command);
    assert!(start.elapsed() < WAIT, "suspended cleanup blocked");
    root.assert_terminated();
    thread::sleep(Duration::from_millis(100));
    assert!(!marker.exists(), "Drop resumed executable user code");
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn rejects_invalid_executables_and_cmd_without_executing_a_marker() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let marker = dir.path().join("must-not-run");
    let script = dir.path().join("fixture.cmd");
    fs::write(&script, format!("@echo ran>\"{}\"\r\n", marker.display())).unwrap();
    let invalid = dir.path().join("invalid.exe");
    fs::write(&invalid, b"not a native executable").unwrap();
    let disguised = dir.path().join("native-but-not-allowed.cmd");
    fs::copy(fixture(), &disguised).unwrap();
    for executable in [
        script,
        invalid,
        disguised,
        dir.path().join("absent.exe"),
        PathBuf::from("cedar-winprocess-fixture.exe"),
    ] {
        let candidate = LaunchSpec {
            executable,
            arguments: vec!["marker".into(), text_path(&marker)],
            cwd: dir.path().to_owned(),
        };
        assert!(WindowsCommand::spawn_suspended(&candidate).is_err());
        assert!(!marker.exists());
    }
    for cwd in [PathBuf::from("."), dir.path().join("missing-directory")] {
        let mut candidate = spec(dir.path(), vec!["marker".into(), text_path(&marker)]);
        candidate.cwd = cwd;
        assert!(WindowsCommand::spawn_suspended(&candidate).is_err());
        assert!(!marker.exists());
    }
    let candidate = spec(dir.path(), vec!["marker".into(), "embedded\0nul".into()]);
    assert!(WindowsCommand::spawn_suspended(&candidate).is_err());
    assert!(!marker.exists());
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn round_trips_empty_unicode_quotes_backslashes_and_child_cwd() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let cwd = dir.path().join("workspace with spaces 雪");
    fs::create_dir(&cwd).unwrap();
    let cwd_before = std::env::current_dir().unwrap();
    let args = vec![
        String::new(),
        "plain".into(),
        "two words".into(),
        "\t\n".into(),
        "雪 café 🚀".into(),
        "\"".into(),
        "inside\"quote".into(),
        "\\".into(),
        "trailing space \\".into(),
        "slashes\\\\\\\"and quote".into(),
        "& | < > ^ %PATH% $(untouched)".into(),
    ];
    let mut arguments = vec!["inspect".into()];
    arguments.extend(args.iter().cloned());
    let mut command = launch(&cwd, arguments);
    command.resume().unwrap();
    let mut output = Output::default();
    assert_eq!(output.complete(&mut command).code, 0);
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut lines = stdout.lines();
    let actual_cwd = decode_hex(lines.next().unwrap().strip_prefix("cwd:").unwrap());
    assert_eq!(
        PathBuf::from(actual_cwd).canonicalize().unwrap(),
        cwd.canonicalize().unwrap()
    );
    let actual_args: Vec<String> = lines
        .map(|line| decode_hex(line.strip_prefix("arg:").unwrap()))
        .collect();
    assert_eq!(actual_args, args);
    assert_eq!(std::env::current_dir().unwrap(), cwd_before);
}

fn decode_hex(text: &str) -> String {
    assert_eq!(text.len() % 2, 0);
    let bytes: Vec<u8> = text
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).expect("fixture hex")
        })
        .collect();
    String::from_utf8(bytes).unwrap()
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn root_exit_is_observation_only_then_owner_cleans_descendant_pipe_holders() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let mut command = launch(dir.path(), vec!["tree-exit".into(), text_path(dir.path())]);
    command.resume().unwrap();
    wait_file(&dir.path().join("tree.ready"));
    let child = ObservedProcess::from_file(&dir.path().join("branch.pid"));
    let grandchild = ObservedProcess::from_file(&dir.path().join("leaf.pid"));
    let mut output = Output::default();
    let deadline = Instant::now() + WAIT;
    loop {
        output.round(&mut command);
        if let Some(exit) = command.try_exit().unwrap() {
            assert_eq!(exit.code, 23);
            break;
        }
        assert!(Instant::now() < deadline, "root did not exit naturally");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(child.alive() && grandchild.alive());
    assert!(command.active_processes().unwrap() >= 2);
    let progress = output.round(&mut command);
    assert!(!progress.stdout_eof && !progress.stderr_eof);
    command.terminate_tree().unwrap();
    child.assert_terminated();
    grandchild.assert_terminated();
    wait_empty_job(&command);
    output.drain(&mut command);
    assert_eq!(command.wait_exit().unwrap().code, 23);
    assert_eq!(command.try_exit().unwrap().unwrap().code, 23);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn cancel_terminates_nested_child_and_grandchild_and_is_repeatable() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let mut command = launch(dir.path(), vec!["tree-live".into(), text_path(dir.path())]);
    command.resume().unwrap();
    wait_file(&dir.path().join("tree.ready"));
    let root = ObservedProcess::from_file(&dir.path().join("root.pid"));
    let child = ObservedProcess::from_file(&dir.path().join("branch.pid"));
    let grandchild = ObservedProcess::from_file(&dir.path().join("leaf.pid"));
    assert_job_accounts_for_live_fixtures(&command, 3);
    command.terminate_tree().unwrap();
    command.terminate_tree().unwrap();
    command.cancel_capture_and_complete().unwrap();
    command.cancel_capture_and_complete().unwrap();
    root.assert_terminated();
    child.assert_terminated();
    grandchild.assert_terminated();
    wait_empty_job(&command);
    assert_ne!(command.wait_exit().unwrap().code, LIFETIME_CAP_EXIT);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn drop_terminates_a_running_nested_tree() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let mut command = launch(dir.path(), vec!["tree-live".into(), text_path(dir.path())]);
    command.resume().unwrap();
    wait_file(&dir.path().join("tree.ready"));
    let root = ObservedProcess::from_file(&dir.path().join("root.pid"));
    let child = ObservedProcess::from_file(&dir.path().join("branch.pid"));
    let grandchild = ObservedProcess::from_file(&dir.path().join("leaf.pid"));
    drop(command);
    root.assert_terminated();
    child.assert_terminated();
    grandchild.assert_terminated();
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn cancels_and_completes_pending_reads_while_writer_is_still_alive() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let ready = dir.path().join("idle.pid");
    let mut command = launch(dir.path(), vec!["idle".into(), text_path(&ready)]);
    command.resume().unwrap();
    let writer = ObservedProcess::from_file(&ready);
    let mut output = Output::default();
    let deadline = Instant::now() + WAIT;
    loop {
        let progress = output.round(&mut command);
        assert!(!progress.stdout_eof && !progress.stderr_eof);
        if output.stdout == b"idle-stdout-ready\n"
            && output.stderr == b"idle-stderr-ready\n"
            && progress.bytes == 0
        {
            // Both streams are drained, so another round has armed pending
            // reads with no available bytes and open writers in a live root.
            break;
        }
        assert!(Instant::now() < deadline, "idle streams not captured");
        thread::sleep(Duration::from_millis(5));
    }
    let before = Instant::now();
    command.cancel_capture_and_complete().unwrap();
    assert!(before.elapsed() < Duration::from_secs(1));
    let stopped = output.round(&mut command);
    assert_eq!(stopped.bytes, 0);
    assert!(!stopped.stdout_eof && !stopped.stderr_eof);
    assert!(writer.alive(), "capture cancellation terminated the writer");
    assert_eq!(command.try_exit().unwrap(), None);
    assert_job_accounts_for_live_fixtures(&command, 1);
    command.cancel_capture_and_complete().unwrap();
    command.terminate_tree().unwrap();
    writer.assert_terminated();
    command.wait_exit().unwrap();
    wait_empty_job(&command);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn simultaneous_stdout_stderr_are_fully_drained_after_fast_exit() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let bytes = 512 * 1024;
    let mut command = launch(
        dir.path(),
        vec!["flood".into(), bytes.to_string(), "259".into()],
    );
    command.resume().unwrap();
    let mut output = Output::default();
    assert_eq!(output.complete(&mut command).code, 259);
    let mut expected_out = vec![b'O'; bytes];
    expected_out.extend_from_slice(b"\nstdout-end\n");
    let mut expected_err = vec![b'E'; bytes];
    expected_err.extend_from_slice(b"\nstderr-end\n");
    assert_eq!(output.stdout, expected_out);
    assert_eq!(output.stderr, expected_err);
    wait_empty_job(&command);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn fast_exit_preserves_all_native_exit_code_bits() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    for expected in [0, 23, 259, 0x8000_0001, 0xffff_ffff] {
        let mut command = launch(dir.path(), vec!["exit".into(), expected.to_string()]);
        command.resume().unwrap();
        assert_eq!(
            Output::default().complete(&mut command).code,
            expected,
            "native exit bits for {expected:#x}"
        );
        assert_eq!(command.try_exit().unwrap().unwrap().code, expected);
        wait_empty_job(&command);
    }
}

struct Sentinel(HANDLE);

impl Sentinel {
    fn inheritable() -> Self {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        // SAFETY: Valid initialized SECURITY_ATTRIBUTES for this call, default
        // security descriptor, no name, manual-reset event initially false.
        // https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createeventw
        let handle = unsafe { CreateEventW(&attributes, 1, 0, std::ptr::null()) };
        assert!(
            !handle.is_null(),
            "CreateEventW: {}",
            io::Error::last_os_error()
        );
        let sentinel = Self(handle);
        let mut flags = 0;
        // SAFETY: sentinel owns a valid handle and flags is a writable DWORD.
        // https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-gethandleinformation
        assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
        assert_ne!(flags & HANDLE_FLAG_INHERIT, 0);
        sentinel
    }
}

impl Drop for Sentinel {
    fn drop(&mut self) {
        // SAFETY: This event handle is owned exclusively by this wrapper.
        unsafe { CloseHandle(self.0) };
    }
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn handle_list_excludes_an_unrelated_inheritable_event() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let sentinel = Sentinel::inheritable();
    let value = sentinel.0 as usize;
    let mut command = launch(dir.path(), vec!["probe-sentinel".into(), value.to_string()]);
    command.resume().unwrap();
    let mut output = Output::default();
    assert_eq!(output.complete(&mut command).code, 0);
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .starts_with(&format!("sentinel-probed:{value}:")));
    // Observe the *parent's event*, not whether this numerical handle happens
    // to have been reused for some unrelated object in the child's table.
    // SAFETY: The event is held open and has SYNCHRONIZE access; timeout is 0.
    assert_eq!(unsafe { WaitForSingleObject(sentinel.0, 0) }, WAIT_TIMEOUT);
}

/// Only owns the directly spawned owner process. Any abnormal cleanup uses
/// std::process::Child's owned OS handle, never OpenProcess or a PID lookup.
struct OwnerGuard(Child);

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn owner_crash_closes_inner_job_and_kills_child_without_outer_job_cleanup() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    // This is deliberately NOT a WindowsCommand. The test does not put the
    // owner or its child in another driver-owned job, nor terminate an outer
    // job to make this test pass. Runner-level CI containment remains untouched.
    let mut owner = OwnerGuard(
        Command::new(fixture())
            .arg("crash-owner")
            .arg(dir.path())
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start independent owner fixture"),
    );
    wait_file(&dir.path().join("owner.ready"));
    assert!(owner.0.try_wait().unwrap().is_none());
    let child = ObservedProcess::from_file(&dir.path().join("owned-child.pid"));
    assert!(child.alive());
    fs::write(dir.path().join("owner.crash"), b"crash now").unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = owner.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(79), "owner did not take crash path");
            break;
        }
        assert!(Instant::now() < deadline, "owner did not exit on request");
        thread::sleep(Duration::from_millis(5));
    }
    // Assert BEFORE OwnerGuard cleanup: that guard cannot mask inner-job
    // failure. The read-only process handle does not keep the job handle open.
    child.assert_terminated();
}

fn handle_count() -> u32 {
    let mut count = 0;
    // SAFETY: GetCurrentProcess yields a pseudo handle with query access;
    // count is writable. Pseudo handles are never closed by this helper.
    // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesshandlecount
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
        0
    );
    count
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn repeated_suspended_and_completed_cleanup_does_not_grow_handle_count() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    // Warm up runtime/library one-time allocations before taking a baseline.
    cleanup_cycle(dir.path());
    let baseline = handle_count();
    for _ in 0..12 {
        cleanup_cycle(dir.path());
    }
    let after_first_batch = handle_count();
    for _ in 0..12 {
        cleanup_cycle(dir.path());
    }
    let final_count = handle_count();
    // A tiny fixed allowance tolerates unrelated runtime bookkeeping, but a
    // per-launch leak grows well beyond it across 48 fresh child launches.
    assert!(
        after_first_batch <= baseline + 2 && final_count <= baseline + 2,
        "handle growth: baseline={baseline}, batch1={after_first_batch}, batch2={final_count}"
    );
}

fn cleanup_cycle(dir: &Path) {
    drop(launch(dir, vec!["exit".into(), "0".into()]));
    let mut command = launch(dir, vec!["exit".into(), "0".into()]);
    command.resume().unwrap();
    assert_eq!(Output::default().complete(&mut command).code, 0);
    command.terminate_tree().unwrap();
    command.cancel_capture_and_complete().unwrap();
    command.cancel_capture_and_complete().unwrap();
    wait_empty_job(&command);
}

fn launch_piped(dir: &Path, arguments: Vec<String>) -> WindowsCommand {
    WindowsCommand::spawn_suspended_with_piped_stdin(&spec(dir, arguments))
        .expect("spawn suspended fixture with piped stdin")
}

fn finish_write(command: &mut WindowsCommand, mut progress: StdinWriteProgress) -> usize {
    let deadline = Instant::now() + WAIT;
    loop {
        match progress {
            StdinWriteProgress::Written(n) => return n,
            StdinWriteProgress::Pending => {}
            other => panic!("write lost its completion: {other:?}"),
        }
        assert!(Instant::now() < deadline, "stdin write did not complete");
        thread::sleep(Duration::from_millis(1));
        progress = command.poll_stdin_write().unwrap();
    }
}

fn send_input(command: &mut WindowsCommand, input: &[u8]) {
    let mut remaining = input;
    while !remaining.is_empty() {
        let chunk = &remaining[..remaining.len().min(MAX_STDIN_WRITE_BYTES)];
        let progress = command.begin_stdin_write(chunk).unwrap();
        let written = finish_write(command, progress);
        assert!(written > 0 && written <= chunk.len());
        remaining = &remaining[written..];
    }
    assert_eq!(
        command.poll_stdin_write().unwrap(),
        StdinWriteProgress::Idle
    );
}

fn fill_stdin(command: &mut WindowsCommand) {
    let bytes = vec![b'x'; MAX_STDIN_WRITE_BYTES];
    for _ in 0..16 {
        match command.begin_stdin_write(&bytes).unwrap() {
            StdinWriteProgress::Written(n) => assert!(n > 0 && n <= bytes.len()),
            StdinWriteProgress::Pending => match command.poll_stdin_write().unwrap() {
                StdinWriteProgress::Pending => return,
                // Async acceptance may finish between begin and poll. Account
                // for it exactly once and continue the bounded capacity fill.
                StdinWriteProgress::Written(n) => assert!(n > 0 && n <= bytes.len()),
                other => panic!("unexpected fill completion: {other:?}"),
            },
            other => panic!("unexpected fill state: {other:?}"),
        }
    }
    panic!("nonreading fixture did not block stdin within 1 MiB");
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn piped_stdin_round_trips_multiple_chunks_binary_bytes_and_eof_exactly() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let mut command = launch_piped(dir.path(), vec!["stdin-echo".into()]);
    // Job membership still precedes resume in the new constructor.
    assert_eq!(command.active_processes().unwrap(), 1);
    assert_eq!(
        command.poll_stdin_write().unwrap(),
        StdinWriteProgress::Idle
    );
    assert_eq!(
        command
            .begin_stdin_write(&vec![0; MAX_STDIN_WRITE_BYTES + 1])
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        command.begin_stdin_write(&[]).unwrap(),
        StdinWriteProgress::Written(0)
    );
    command.resume().unwrap();
    let input: Vec<u8> = (0..MAX_STDIN_WRITE_BYTES * 3 + 17)
        .map(|index| (index % 256) as u8)
        .collect();
    send_input(&mut command, &input);
    command.close_stdin().unwrap();
    command.close_stdin().unwrap();
    assert_eq!(
        command.poll_stdin_write().unwrap(),
        StdinWriteProgress::Closed
    );
    assert_eq!(
        command.begin_stdin_write(b"closed").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    let mut output = Output::default();
    assert_eq!(output.complete(&mut command).code, 0);
    assert_eq!(output.stdout, [input.as_slice(), b"\nstdin-eof\n"].concat());
    assert_eq!(
        output.stderr,
        format!("stdin-bytes:{}\n", input.len()).as_bytes()
    );
    wait_empty_job(&command);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn piped_stdin_empty_close_and_default_nul_both_deliver_eof() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let mut piped = launch_piped(dir.path(), vec!["null-stdin".into()]);
    piped.close_stdin().unwrap(); // EOF is valid even before resume
    piped.close_stdin().unwrap();
    piped.resume().unwrap();
    let mut output = Output::default();
    assert_eq!(output.complete(&mut piped).code, 0);
    assert_eq!(output.stdout, b"stdin-eof\n");
    wait_empty_job(&piped);
    let mut nul = launch(dir.path(), vec!["null-stdin".into()]);
    assert_eq!(nul.poll_stdin_write().unwrap(), StdinWriteProgress::Closed);
    assert_eq!(
        nul.begin_stdin_write(b"not piped").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    nul.close_stdin().unwrap();
    assert_eq!(
        nul.cancel_stdin_and_complete().unwrap(),
        StdinCancelOutcome::Idle
    );
    nul.resume().unwrap();
    let mut output = Output::default();
    assert_eq!(output.complete(&mut nul).code, 0);
    assert_eq!(output.stdout, b"stdin-eof\n");
    wait_empty_job(&nul);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn piped_stdin_full_pipe_cancels_while_child_lives_and_close_is_repeatable() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let ready = dir.path().join("idle.pid");
    let mut command = launch_piped(dir.path(), vec!["idle".into(), text_path(&ready)]);
    command.resume().unwrap();
    let root = ObservedProcess::from_file(&ready);
    let original_pid = command.process_id();
    assert_eq!(
        original_pid,
        fs::read_to_string(&ready).unwrap().parse::<u32>().unwrap()
    );
    // SAFETY: this held observation handle has query rights and identifies the
    // already-observed fixture. The diagnostic PID is never used for cleanup.
    // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessid
    let observed_pid = unsafe { GetProcessId(root.0.as_raw_handle()) };
    assert_ne!(observed_pid, 0, "{}", io::Error::last_os_error());
    assert_eq!(original_pid, observed_pid);
    fill_stdin(&mut command);
    assert_eq!(
        command
            .begin_stdin_write(b"no replacement")
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        command.close_stdin().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    let started = Instant::now();
    assert_eq!(
        command.cancel_stdin_and_complete().unwrap(),
        StdinCancelOutcome::Cancelled
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(root.alive(), "cancelling stdin terminated the live reader");
    command.close_stdin().unwrap();
    assert_eq!(
        command.cancel_stdin_and_complete().unwrap(),
        StdinCancelOutcome::Idle
    );
    assert_eq!(
        command.poll_stdin_write().unwrap(),
        StdinWriteProgress::Closed
    );
    command.terminate_tree().unwrap();
    let mut output = Output::default();
    assert_ne!(output.complete(&mut command).code, LIFETIME_CAP_EXIT);
    assert_eq!(output.stdout, b"idle-stdout-ready\n");
    assert_eq!(output.stderr, b"idle-stderr-ready\n");
    root.assert_terminated();
    wait_empty_job(&command);
    assert_eq!(command.process_id(), original_pid); // identity survives exit
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn piped_stdin_exit_race_reports_completion_once_then_refuses_closed_peer_writes() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let mut command = launch_piped(dir.path(), vec!["exit".into(), "23".into()]);
    fill_stdin(&mut command); // pending before any fixture code runs
    command.resume().unwrap();
    let deadline = Instant::now() + WAIT;
    let completion = loop {
        match command.poll_stdin_write() {
            Ok(StdinWriteProgress::Pending) => {}
            result => break result,
        }
        assert!(
            Instant::now() < deadline,
            "pending write survived child exit"
        );
        thread::sleep(Duration::from_millis(1));
    };
    use windows_sys::Win32::Foundation::{
        ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED,
    };
    let assert_disconnected = |error: io::Error| {
        assert!(
            matches!(
                error.raw_os_error().map(|n| n as u32),
                Some(ERROR_BROKEN_PIPE | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED)
            ),
            "unexpected write failure: {error}"
        );
    };
    let completed = match completion {
        Ok(StdinWriteProgress::Written(n)) => {
            // Pending is an async observation, not proof that acceptance cannot
            // win the race. Completed bytes do not mean the child read them.
            assert!(n > 0 && n <= MAX_STDIN_WRITE_BYTES);
            assert_eq!(
                command.poll_stdin_write().unwrap(),
                StdinWriteProgress::Idle
            );
            true
        }
        Err(error) => {
            assert_disconnected(error);
            false
        }
        other => panic!("invalid terminal write outcome: {other:?}"),
    };
    assert_eq!(command.wait_exit().unwrap().code, 23);
    if completed {
        // A distinct probe only AFTER owned-handle exit evidence. Never replay
        // the accepted payload or turn a real completion into invented failure.
        let deadline = Instant::now() + WAIT;
        let mut progress = command.begin_stdin_write(b"closed-peer probe");
        loop {
            match progress {
                Err(error) => {
                    assert_disconnected(error);
                    break;
                }
                Ok(StdinWriteProgress::Pending) => {}
                other => panic!("write to known exited peer succeeded: {other:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "closed-peer probe did not complete"
            );
            thread::sleep(Duration::from_millis(1));
            progress = command.poll_stdin_write();
        }
    }
    assert_eq!(
        command.poll_stdin_write().unwrap(),
        StdinWriteProgress::Closed
    );
    assert_eq!(
        command.begin_stdin_write(b"no replay").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    command.close_stdin().unwrap();
    command.close_stdin().unwrap();
    assert_eq!(
        command.cancel_stdin_and_complete().unwrap(),
        StdinCancelOutcome::Idle
    );
    assert_eq!(Output::default().complete(&mut command).code, 23);
    wait_empty_job(&command);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn piped_stdin_drop_joins_pending_input_and_output_and_terminates_owned_tree() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let mut command = launch_piped(dir.path(), vec!["tree-live".into(), text_path(dir.path())]);
    command.resume().unwrap();
    wait_file(&dir.path().join("tree.ready"));
    let root = ObservedProcess::from_file(&dir.path().join("root.pid"));
    let child = ObservedProcess::from_file(&dir.path().join("branch.pid"));
    let grandchild = ObservedProcess::from_file(&dir.path().join("leaf.pid"));
    assert_job_accounts_for_live_fixtures(&command, 3);
    fill_stdin(&mut command);
    let mut output = Output::default();
    output.round(&mut command);
    let progress = output.round(&mut command);
    assert!(!progress.stdout_eof && !progress.stderr_eof);
    let started = Instant::now();
    drop(command);
    assert!(
        started.elapsed() < WAIT,
        "pending stdin delayed tree cleanup"
    );
    root.assert_terminated();
    child.assert_terminated();
    grandchild.assert_terminated();
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn piped_stdin_exact_handles_exclude_sentinel_and_allow_independent_eof() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    let sentinel = Sentinel::inheritable();
    let mut first = launch_piped(
        dir.path(),
        vec!["stdin-echo".into(), (sentinel.0 as usize).to_string()],
    );
    let ready = dir.path().join("second.pid");
    let mut second = launch_piped(dir.path(), vec!["idle".into(), text_path(&ready)]);
    first.resume().unwrap();
    second.resume().unwrap();
    let second_root = ObservedProcess::from_file(&ready);
    send_input(&mut first, b"first input only");
    first.close_stdin().unwrap();
    let mut output = Output::default();
    assert_eq!(output.complete(&mut first).code, 0);
    let newline = output
        .stdout
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap();
    assert!(output.stdout[..newline]
        .starts_with(format!("sentinel-probed:{}:", sentinel.0 as usize).as_bytes()));
    assert_eq!(
        &output.stdout[newline + 1..],
        b"first input only\nstdin-eof\n"
    );
    assert_eq!(output.stderr, b"stdin-bytes:16\n");
    // SAFETY: the held event is queried only; signaling this exact parent's
    // object would prove a leaked handle even if the child reuses the number.
    assert_eq!(unsafe { WaitForSingleObject(sentinel.0, 0) }, WAIT_TIMEOUT);
    assert!(second_root.alive(), "closing first job affected the second");
    fill_stdin(&mut second);
    assert_eq!(
        second.cancel_stdin_and_complete().unwrap(),
        StdinCancelOutcome::Cancelled
    );
    second.terminate_tree().unwrap();
    let mut second_output = Output::default();
    assert_ne!(second_output.complete(&mut second).code, LIFETIME_CAP_EXIT);
    assert_eq!(second_output.stdout, b"idle-stdout-ready\n");
    assert_eq!(second_output.stderr, b"idle-stderr-ready\n");
    second_root.assert_terminated();
    wait_empty_job(&first);
    wait_empty_job(&second);
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn piped_stdin_owner_crash_with_pending_io_kills_child_without_outer_cleanup() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    // As in the NUL-stdin crash test, no driver job can mask inner ownership.
    let mut owner = OwnerGuard(
        Command::new(fixture())
            .arg("crash-owner-piped")
            .arg(dir.path())
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start independent owner with pending stdio"),
    );
    wait_file(&dir.path().join("owner.ready"));
    assert!(owner.0.try_wait().unwrap().is_none());
    let child = ObservedProcess::from_file(&dir.path().join("owned-child.pid"));
    assert!(child.alive());
    fs::write(dir.path().join("owner.crash"), b"crash with pending input").unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(exit) = owner.0.try_wait().unwrap() {
            assert_eq!(exit.code(), Some(79), "owner did not take crash path");
            break;
        }
        assert!(Instant::now() < deadline, "piped owner did not exit");
        thread::sleep(Duration::from_millis(5));
    }
    child.assert_terminated(); // before any OwnerGuard cleanup
}

#[test]
#[ignore = "requires real Windows and CEDAR_WINPROCESS_FIXTURE_BIN"]
fn piped_stdin_repeated_failed_suspended_and_completed_cleanup_does_not_leak_handles() {
    let _watchdog = Watchdog::start();
    let dir = TempDir::new().unwrap();
    piped_cleanup_cycle(dir.path());
    let baseline = handle_count();
    for _ in 0..12 {
        piped_cleanup_cycle(dir.path());
    }
    let first_batch = handle_count();
    for _ in 0..12 {
        piped_cleanup_cycle(dir.path());
    }
    let final_count = handle_count();
    assert!(first_batch <= baseline + 2 && final_count <= baseline + 2,
        "piped-stdin handle growth: baseline={baseline}, batch1={first_batch}, batch2={final_count}");
}

fn piped_cleanup_cycle(dir: &Path) {
    let mut invalid = spec(dir, vec![]);
    invalid.executable = dir.join("missing.exe");
    assert!(WindowsCommand::spawn_suspended_with_piped_stdin(&invalid).is_err());
    let mut suspended = launch_piped(dir, vec!["exit".into(), "0".into()]);
    fill_stdin(&mut suspended);
    let root = ObservedProcess(suspended.observation_handle().unwrap());
    drop(suspended);
    root.assert_terminated();
    let mut command = launch_piped(dir, vec!["stdin-echo".into()]);
    command.resume().unwrap();
    send_input(&mut command, b"one completed write");
    command.close_stdin().unwrap();
    assert_eq!(Output::default().complete(&mut command).code, 0);
    assert_eq!(
        command.cancel_stdin_and_complete().unwrap(),
        StdinCancelOutcome::Idle
    );
    wait_empty_job(&command);
}
