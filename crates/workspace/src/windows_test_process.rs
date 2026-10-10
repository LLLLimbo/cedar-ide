//! Test-only fixture launches for the controlled Windows workspace test host.
//!
//! Every process in this host must use restricted inheritance. An exact handle
//! list protects this child, but cannot protect it from an unrelated concurrent
//! broad-inheritance spawn. This helper changes no production launch policy.

use cedar_winprocess::{CaptureProgress, LaunchSpec, Stream, WindowsCommand};
use std::{
    ffi::{OsStr, OsString},
    fs, io,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(1);
const FINAL_DRAIN_BUDGET: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) code: u32,
}

/// Snapshot ordinary inherited entries without changing the process environment.
/// Windows may include hidden drive-current-directory entries such as '=C:'.
/// They are unnecessary for these absolute executable/cwd launches and cannot
/// occur in the existing owner's strict explicit environment. Omit only those
/// well-formed keys; malformed names remain for that owner to reject.
pub(crate) fn inherited_environment() -> Vec<(OsString, OsString)> {
    without_drive_current_directories(std::env::vars_os())
}

fn without_drive_current_directories(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    environment
        .into_iter()
        .filter(|(name, _)| {
            let mut units = name.encode_wide();
            !matches!(
                (units.next(), units.next(), units.next(), units.next()),
                (Some(61), Some(65..=90 | 97..=122), Some(58), None)
            )
        })
        .collect()
}

/// Run an absolute native executable with NUL stdin and a complete child-local
/// environment. The cap applies independently to each retained output stream.
/// Execution and final capture drain share one budget; exceptional kernel
/// termination, cancellation completion and the owner's Drop are not a hard
/// wall-clock deadline. No worker, shell, global lock or host mutation is used.
pub(crate) fn run(
    executable: &Path,
    arguments: &[String],
    cwd: &Path,
    environment: &[(OsString, OsString)],
    execution_budget: Duration,
    output_cap_per_stream: usize,
) -> io::Result<Output> {
    run_observed(
        &LaunchSpec {
            executable: executable.to_owned(),
            arguments: arguments.to_owned(),
            cwd: cwd.to_owned(),
        },
        environment,
        execution_budget,
        output_cap_per_stream,
        |_| Ok(()),
    )
}

// The observation hook lets these tests retain the existing owner's read-only
// handle before any outcome, including unwinding. It adds no process primitive.
fn run_observed(
    spec: &LaunchSpec,
    environment: &[(OsString, OsString)],
    execution_budget: Duration,
    output_cap_per_stream: usize,
    observe: impl FnOnce(&WindowsCommand) -> io::Result<()>,
) -> io::Result<Output> {
    let deadline = Instant::now()
        .checked_add(execution_budget)
        .ok_or_else(|| invalid("fixture execution budget is too large"))?;
    let mut owner = WindowsCommand::spawn_suspended_with_environment(
        spec,
        environment.iter().map(|(name, value)| (name, value)),
    )?;
    let mut capture = Capture::default();
    let execution = (|| {
        if Instant::now() >= deadline {
            return Err(timeout());
        }
        owner.resume()?;
        observe(&owner)?;
        loop {
            if Instant::now() >= deadline {
                return Err(timeout());
            }
            capture.round(&mut owner, output_cap_per_stream)?;
            if owner.try_exit()?.is_some() {
                return Ok(());
            }
            thread::sleep(POLL_INTERVAL);
        }
    })();

    let mut failure = execution.err();
    // Root exit is not tree exit. Stop the entire owned job before final drain,
    // cancellation or wait, and attempt every cleanup step even after an error.
    if let Err(error) = owner.terminate_tree() {
        failure.get_or_insert(error);
    }
    if failure.is_none() {
        let drain_deadline = deadline.min(Instant::now() + FINAL_DRAIN_BUDGET);
        while !capture.done() {
            if Instant::now() >= drain_deadline {
                failure = Some(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fixture output did not reach EOF within the final drain budget",
                ));
                break;
            }
            if let Err(error) = capture.round(&mut owner, output_cap_per_stream) {
                failure = Some(error);
                break;
            }
            if !capture.done() {
                thread::sleep(POLL_INTERVAL);
            }
        }
    }
    if let Err(error) = owner.cancel_capture_and_complete() {
        failure.get_or_insert(error);
    }
    let exit = owner.wait_exit();
    // Drop also joins owned job cleanup; it protects early errors and panics.
    // Never publish output or an error while the process owner is still alive.
    drop(owner);
    if failure.is_none() && Instant::now() >= deadline {
        failure = Some(timeout());
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(Output {
        stdout: capture.stdout,
        stderr: capture.stderr,
        code: exit?.code,
    })
}

#[derive(Default)]
struct Capture {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    progress: CaptureProgress,
}

impl Capture {
    fn done(&self) -> bool {
        self.progress.stdout_eof && self.progress.stderr_eof
    }

    fn round(&mut self, owner: &mut WindowsCommand, cap: usize) -> io::Result<()> {
        let mut capped = false;
        self.progress = owner.capture_round(|stream, bytes| {
            let target = match stream {
                Stream::Stdout => &mut self.stdout,
                Stream::Stderr => &mut self.stderr,
            };
            let available = cap.saturating_sub(target.len());
            target.extend_from_slice(&bytes[..available.min(bytes.len())]);
            capped |= bytes.len() > available;
        })?;
        if capped {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fixture exceeded its per-stream output cap",
            ))
        } else {
            Ok(())
        }
    }
}

/// Resolve the compiler without spawning a shell or asking rustup. An explicit
/// RUSTC must name an existing absolute .exe. Otherwise only absolute PATH
/// directories are searched, and ambiguous distinct rustc.exe files fail closed.
pub(crate) fn resolve_rustc(environment: &[(OsString, OsString)]) -> io::Result<PathBuf> {
    if let Some(rustc) = environment_value(environment, "RUSTC")? {
        return canonical_executable(Path::new(rustc));
    }
    let path = environment_value(environment, "PATH")?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "fixture compiler PATH is absent")
    })?;
    let mut selected = None;
    for directory in std::env::split_paths(path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join("rustc.exe");
        if !candidate.is_file() {
            continue;
        }
        let candidate = canonical_executable(&candidate)?;
        match &selected {
            Some(previous) if previous != &candidate => {
                return Err(invalid(
                    "multiple fixture compilers exist in PATH; set an absolute RUSTC",
                ));
            }
            None => selected = Some(candidate),
            _ => {}
        }
    }
    selected.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no absolute native fixture compiler found; set RUSTC",
        )
    })
}

fn environment_value<'a>(
    environment: &'a [(OsString, OsString)],
    key: &str,
) -> io::Result<Option<&'a OsStr>> {
    let mut selected = None;
    for (name, value) in environment {
        if environment_key_eq(name, key)? {
            if selected.is_some() {
                return Err(invalid("duplicate fixture compiler environment variable"));
            }
            selected = Some(value.as_os_str());
        }
    }
    Ok(selected)
}

/// Match the OS's environment-name semantics without converting retained
/// values or names through lossy UTF-8, including non-ASCII ordinal aliases.
pub(crate) fn environment_key_eq(name: &OsStr, target: &str) -> io::Result<bool> {
    use windows_sys::Win32::Globalization::{
        CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
    };
    let name: Vec<_> = name.encode_wide().collect();
    let target: Vec<_> = target.encode_utf16().collect();
    if name.is_empty() || target.is_empty() {
        return Ok(name.is_empty() && target.is_empty());
    }
    let name_len =
        i32::try_from(name.len()).map_err(|_| invalid("environment name is too long"))?;
    let target_len =
        i32::try_from(target.len()).map_err(|_| invalid("environment name is too long"))?;
    // SAFETY: these live UTF-16 slices have explicit nonzero lengths that fit
    // the API. Comparison uses native ordinal ignore-case semantics, not locale.
    match unsafe { CompareStringOrdinal(name.as_ptr(), name_len, target.as_ptr(), target_len, 1) } {
        CSTR_EQUAL => Ok(true),
        CSTR_LESS_THAN | CSTR_GREATER_THAN => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

fn canonical_executable(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute()
        || !path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        || !path
            .to_str()
            .is_some_and(|text| !text.contains(['\0', '"']))
    {
        return Err(invalid(
            "fixture compiler must be an absolute UTF-8 .exe path",
        ));
    }
    let canonical = fs::canonicalize(path)?;
    if !canonical.is_file()
        || !canonical
            .to_str()
            .is_some_and(|text| !text.contains(['\0', '"']))
        || !canonical
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err(invalid("fixture compiler must be an existing UTF-8 file"));
    }
    Ok(canonical)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn timeout() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "fixture execution budget expired")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        os::windows::{
            ffi::OsStringExt,
            io::{AsRawHandle, OwnedHandle},
        },
        panic::{catch_unwind, AssertUnwindSafe},
        sync::mpsc,
    };
    use windows_sys::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::WaitForSingleObject,
    };

    const CHILD_TEST: &str = "windows_test_process::tests::fixture_child";
    const MODE: &str = "CEDAR_WORKSPACE_PROCESS_FIXTURE_MODE";
    const TOKEN: &str = "CEDAR_WORKSPACE_PROCESS_FIXTURE_TOKEN";
    const START: &[u8] = b"CEDAR_WORKSPACE_FIXTURE_OUTPUT\n";
    const WAIT: Duration = Duration::from_secs(5);

    fn fixture_spec(root: &Path) -> LaunchSpec {
        LaunchSpec {
            executable: std::env::current_exe().unwrap(),
            arguments: ["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"]
                .map(str::to_owned)
                .to_vec(),
            cwd: root.to_owned(),
        }
    }

    fn environment(mode: &str, token: &str) -> Vec<(OsString, OsString)> {
        let mut values: Vec<_> = inherited_environment()
            .into_iter()
            .filter(|(name, _)| {
                !environment_key_eq(name, MODE).unwrap()
                    && !environment_key_eq(name, TOKEN).unwrap()
            })
            .collect();
        values.push((MODE.into(), mode.into()));
        values.push((TOKEN.into(), token.into()));
        values
    }

    fn assert_exited(handle: &OwnedHandle) {
        // SAFETY: this non-inheritable observation handle belongs to the exact
        // launched process; this zero-duration wait only observes its state.
        assert_eq!(
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
    }

    fn payload(stdout: &[u8]) -> &[u8] {
        let offset = stdout
            .windows(START.len())
            .position(|window| window == START)
            .expect("child output marker missing");
        &stdout[offset + START.len()..]
    }

    #[test]
    fn fixture_child() {
        let Some(mode) = std::env::var_os(MODE) else {
            return;
        };
        // The environment alone must never turn a whole test-suite invocation
        // into a child fixture. This branch also requires its exact test name.
        let args: Vec<_> = std::env::args().collect();
        assert!(args.windows(2).any(|args| args == ["--exact", CHILD_TEST]));
        thread::spawn(|| {
            thread::sleep(Duration::from_secs(10));
            std::process::exit(124);
        });
        let token = std::env::var(TOKEN).unwrap();
        match mode.to_str().unwrap() {
            "output" => {
                let mut out = io::stdout().lock();
                out.write_all(START).unwrap();
                write!(
                    out,
                    "{}\n{}",
                    std::env::current_dir().unwrap().display(),
                    token
                )
                .unwrap();
                out.flush().unwrap();
                io::stderr().write_all(b"fixture stderr\n").unwrap();
                std::process::exit(37);
            }
            "exit" => std::process::exit(token.parse::<u32>().unwrap() as i32),
            "sleep" => loop {
                thread::sleep(Duration::from_millis(10));
            },
            "flood" => loop {
                io::stdout().write_all(&[b'o'; 8192]).unwrap();
                io::stderr().write_all(&[b'e'; 8192]).unwrap();
            },
            "concurrent" => {
                fs::write(format!("ready-{token}"), []).unwrap();
                let deadline = Instant::now() + WAIT;
                while !Path::new("ready-a").is_file() || !Path::new("ready-b").is_file() {
                    assert!(Instant::now() < deadline, "concurrent child did not start");
                    thread::sleep(POLL_INTERVAL);
                }
                let byte = token.as_bytes()[0];
                let err = thread::spawn(move || {
                    io::stderr().write_all(&vec![byte; 48 * 1024]).unwrap();
                });
                let mut out = io::stdout().lock();
                out.write_all(START).unwrap();
                out.write_all(&vec![byte; 48 * 1024]).unwrap();
                out.flush().unwrap();
                err.join().unwrap();
                if token == "b" {
                    let deadline = Instant::now() + WAIT;
                    while !Path::new("release-b").is_file() {
                        assert!(Instant::now() < deadline, "short capture did not finish");
                        thread::sleep(POLL_INTERVAL);
                    }
                }
                std::process::exit(0);
            }
            _ => panic!("unknown fixture mode"),
        }
    }

    #[test]
    fn captures_both_streams_exit_cwd_and_explicit_environment() {
        let root = tempfile::tempdir().unwrap();
        let spec = fixture_spec(root.path());
        let token = "literal % ! ^ & | < > \" Unicode 雪";
        let result = run(
            &spec.executable,
            &spec.arguments,
            &spec.cwd,
            &environment("output", token),
            WAIT,
            32 * 1024,
        )
        .unwrap();
        assert_eq!(result.code, 37);
        assert_eq!(result.stderr, b"fixture stderr\n");
        let actual = String::from_utf8(payload(&result.stdout).to_vec()).unwrap();
        let (cwd, actual_token) = actual.split_once('\n').unwrap();
        assert_eq!(
            fs::canonicalize(cwd).unwrap(),
            fs::canonicalize(root.path()).unwrap()
        );
        assert_eq!(actual_token, token);
        for code in [259, 0xdead_beef] {
            let result = run(
                &spec.executable,
                &spec.arguments,
                &spec.cwd,
                &environment("exit", &code.to_string()),
                WAIT,
                32 * 1024,
            )
            .unwrap();
            assert_eq!(result.code, code);
        }
    }

    #[test]
    fn timeout_and_output_cap_join_the_owned_process() {
        let root = tempfile::tempdir().unwrap();
        let spec = fixture_spec(root.path());
        for (mode, budget, cap, expected) in [
            (
                "sleep",
                Duration::from_millis(300),
                32 * 1024,
                io::ErrorKind::TimedOut,
            ),
            ("flood", WAIT, 1024, io::ErrorKind::InvalidData),
        ] {
            let mut observation = None;
            let error = run_observed(&spec, &environment(mode, ""), budget, cap, |owner| {
                observation = Some(owner.observation_handle()?);
                Ok(())
            })
            .unwrap_err();
            assert_eq!(error.kind(), expected);
            assert_exited(observation.as_ref().unwrap());
        }
    }

    #[test]
    fn errors_and_panics_still_join_the_owned_process() {
        let root = tempfile::tempdir().unwrap();
        let spec = fixture_spec(root.path());
        let mut observation = None;
        let error = run_observed(&spec, &environment("sleep", ""), WAIT, 1024, |owner| {
            observation = Some(owner.observation_handle()?);
            Err(io::Error::other("injected fixture observer failure"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "injected fixture observer failure");
        assert_exited(observation.as_ref().unwrap());
        observation = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = run_observed(&spec, &environment("sleep", ""), WAIT, 1024, |owner| {
                observation = Some(owner.observation_handle()?);
                panic!("injected fixture observer panic");
            });
        }));
        assert!(result.is_err());
        assert_exited(observation.as_ref().unwrap());
    }

    #[test]
    fn concurrent_captures_are_independent_and_drain_both_streams() {
        let root = tempfile::tempdir().unwrap();
        let spec = fixture_spec(root.path());
        thread::scope(|scope| {
            let (sender, receiver) = mpsc::channel();
            let held_spec = &spec;
            let held = scope.spawn(move || {
                run_observed(
                    held_spec,
                    &environment("concurrent", "b"),
                    WAIT,
                    64 * 1024,
                    |owner| {
                        sender.send(owner.observation_handle()?).unwrap();
                        Ok(())
                    },
                )
                .unwrap()
            });
            let observation = receiver.recv_timeout(WAIT).unwrap();
            let short = run(
                &spec.executable,
                &spec.arguments,
                &spec.cwd,
                &environment("concurrent", "a"),
                WAIT,
                64 * 1024,
            )
            .unwrap();
            assert_eq!(short.code, 0);
            assert_eq!(payload(&short.stdout), vec![b'a'; 48 * 1024]);
            assert_eq!(short.stderr, vec![b'a'; 48 * 1024]);
            // SAFETY: observe the sibling's owned read-only handle. Its root
            // must still be alive while the short child's streams reached EOF.
            assert_eq!(
                unsafe { WaitForSingleObject(observation.as_raw_handle(), 0) },
                WAIT_TIMEOUT
            );
            fs::write(root.path().join("release-b"), []).unwrap();
            let held = held.join().unwrap();
            assert_eq!(held.code, 0);
            assert_eq!(payload(&held.stdout), vec![b'b'; 48 * 1024]);
            assert_eq!(held.stderr, vec![b'b'; 48 * 1024]);
            assert_exited(&observation);
        });
    }

    #[test]
    fn compiler_resolution_is_absolute_explicit_and_unambiguous() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(first.join("rustc.exe"), []).unwrap();
        fs::write(second.join("rustc.exe"), []).unwrap();
        let canonical = fs::canonicalize(first.join("rustc.exe")).unwrap();
        let paths = |dirs: &[&Path]| vec![("PATH".into(), std::env::join_paths(dirs).unwrap())];
        assert_eq!(resolve_rustc(&paths(&[&first, &first])).unwrap(), canonical);
        assert_eq!(
            resolve_rustc(&paths(&[&first, &second]))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let mut explicit = paths(&[&first, &second]);
        explicit.push(("RUSTC".into(), first.join("rustc.exe").into_os_string()));
        assert_eq!(resolve_rustc(&explicit).unwrap(), canonical);
        assert!(resolve_rustc(&[("RUSTC".into(), "rustc.exe".into())]).is_err());
        assert!(resolve_rustc(&[("RUSTC".into(), root.path().into())]).is_err());
        assert!(resolve_rustc(&paths(&[Path::new("relative")])).is_err());
        explicit.push(("rustc".into(), second.join("rustc.exe").into_os_string()));
        assert!(resolve_rustc(&explicit).is_err());
    }

    #[test]
    fn environment_snapshot_omits_only_drive_cwd_keys_and_preserves_utf16() {
        let ordinary = vec![
            (OsString::from("PATH"), OsString::from(r"C:\native\bin")),
            (
                OsString::from_wide(&[0xd800, 65]),
                OsString::from_wide(&[0xdc00, 66]),
            ),
            (OsString::from("empty"), OsString::new()),
        ];
        let malformed: Vec<(OsString, OsString)> =
            ["=", "=C", "=C::", "=1:", "=CC:", "=C:extra", "a=b"]
                .map(|name| (name.into(), "retained".into()))
                .to_vec();
        let mut source = ordinary.clone();
        source.extend(malformed.clone());
        for name in ["=A:", "=Z:", "=c:"] {
            source.push((name.into(), r"C:\somewhere".into()));
        }
        let mut expected = ordinary;
        expected.extend(malformed.clone());
        assert_eq!(without_drive_current_directories(source), expected);
        let root = tempfile::tempdir().unwrap();
        let spec = fixture_spec(root.path());
        for entry in malformed {
            let error = run(
                &spec.executable,
                &spec.arguments,
                &spec.cwd,
                &[entry],
                WAIT,
                1024,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        assert!(environment_key_eq(OsStr::new("Path"), "PATH").unwrap());
        assert!(environment_key_eq(OsStr::new("ä"), "Ä").unwrap());
        assert!(!environment_key_eq(OsStr::new("PATH_EXTRA"), "PATH").unwrap());
    }
}
