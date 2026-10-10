//! Nonshipping fixed-sibling route negatives. Every executable, mode file and
//! attempted write belongs to this invocation's generated directory. Fault
//! peers use the opt-in probe's own native ELF; they never run tools or network.
#![cfg(all(target_os = "linux", feature = "fixtures"))]

use std::{
    fs,
    io::{self, Read},
    os::fd::AsRawFd,
    os::unix::{ffi::OsStringExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const ROOT: &str = "workspace café 雪";
const CASES: &[(&str, bool)] = &[
    ("missing", false),
    ("directory", false),
    ("symlink", false),
    ("dangling_symlink", false),
    ("nonexecutable", false),
    ("invalid_elf", false),
    ("wrong_elf_arch", false),
    ("missing_root", false),
    ("non_utf8_root", false),
    ("protocol", true),
    ("missing_metadata", true),
    ("invalid_metadata", true),
    ("version", true),
    ("platform", true),
    ("architecture", true),
    ("root_mismatch", true),
    ("eof", true),
    ("response_id", true),
    ("cancel", true),
    ("pre_cancel", false),
];
const PROBE_BOUND: Duration = Duration::from_secs(15);
const OUTPUT_BOUND: u64 = 4096;

type ProbeResult<T> = Result<T, &'static str>;

fn copy_executable(source: &Path, target: &Path) -> ProbeResult<()> {
    let metadata = fs::symlink_metadata(source).map_err(|_| "probe_source")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("probe_source");
    }
    fs::copy(source, target).map_err(|_| "probe_copy")?;
    fs::set_permissions(target, fs::Permissions::from_mode(0o755)).map_err(|_| "probe_mode")
}

fn nonblocking(pipe: &impl AsRawFd) -> ProbeResult<()> {
    let descriptor = pipe.as_raw_fd();
    // SAFETY: this is the live, uniquely owned subprocess pipe descriptor.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 {
        return Err("probe_pipe_mode");
    }
    // SAFETY: preserve its flags while adding the documented nonblocking flag.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err("probe_pipe_mode");
    }
    Ok(())
}

fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>) -> ProbeResult<bool> {
    let remaining = OUTPUT_BOUND as usize - bytes.len();
    let mut buffer = [0u8; 1024];
    let limit = buffer.len().min(remaining + 1);
    match pipe.read(&mut buffer[..limit]) {
        Ok(0) => Ok(true),
        Ok(read) if read <= remaining => {
            bytes.extend_from_slice(&buffer[..read]);
            Ok(false)
        }
        Ok(_) => Err("probe_output_bound_cleanup_unverified"),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(false),
        Err(_) => Err("probe_output"),
    }
}

fn collect_probe(child: &mut std::process::Child) -> ProbeResult<(Vec<u8>, Vec<u8>)> {
    let mut stdout = child.stdout.take().ok_or("probe_stdout")?;
    let mut stderr = child.stderr.take().ok_or("probe_stderr")?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let mut stdout_eof = false;
    let mut stderr_eof = false;
    let deadline = Instant::now() + PROBE_BOUND;
    loop {
        if Instant::now() >= deadline {
            return Err("probe_timeout_cleanup_unverified");
        }
        if !stdout_eof {
            stdout_eof = drain(&mut stdout, &mut output)?;
        }
        if !stderr_eof {
            stderr_eof = drain(&mut stderr, &mut errors)?;
        }
        if let Some(status) = child.try_wait().map_err(|_| "probe_wait")? {
            if stdout_eof && stderr_eof {
                if !status.success() {
                    return Err("probe_failed");
                }
                if Instant::now() >= deadline {
                    return Err("probe_timeout_cleanup_unverified");
                }
                return Ok((output, errors));
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn verify_receipt(bytes: &[u8], case: &str, spawned: bool) -> ProbeResult<()> {
    let receipt: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "receipt_json")?;
    let fields = receipt.as_object().ok_or("receipt_schema")?;
    let elapsed = receipt["elapsed_ms"].as_u64().ok_or("receipt_elapsed")?;
    let expected = serde_json::json!({
        "kind": "cedar_linux_desktop_rejection_probe",
        "schema_version": 1,
        "status": "success",
        "case": case,
        "rejected": true,
        "trust_off": true,
        "child_spawned": spawned,
        "cleanup_verified": spawned,
        "no_child": !spawned,
        "elapsed_ms": elapsed,
        "elapsed_bound_ms": 10000,
    });
    if fields.len() != 11 || receipt != expected || elapsed > 10000 {
        return Err("receipt_schema");
    }
    Ok(())
}

fn verify_case(source: &Path, case: &str, spawned: bool) -> ProbeResult<()> {
    let scratch = tempfile::Builder::new()
        .prefix("cedar desktop negatives 雪 ")
        .tempdir()
        .map_err(|_| "fixture_create")?;
    let install = scratch.path().join("renamed installation café 雪");
    let decoy = scratch.path().join("misleading cwd and PATH");
    let root = scratch.path().join(ROOT);
    for directory in [&install, &decoy, &root] {
        fs::create_dir(directory).map_err(|_| "fixture_create")?;
    }
    fs::write(
        root.join(".cedar-linux-desktop-acceptance"),
        b"cedar-linux-desktop-acceptance-v1\n",
    )
    .map_err(|_| "fixture_marker")?;
    fs::write(root.join(".cedar-linux-desktop-fault"), case).map_err(|_| "fixture_mode")?;
    fs::create_dir(
        scratch
            .path()
            .join(std::ffi::OsString::from_vec(b"nonutf8-\xff".to_vec())),
    )
    .map_err(|_| "fixture_non_utf8")?;
    let probe = install.join("renamed-client-probe");
    let agent = install.join("cedar-agent");
    copy_executable(source, &probe)?;
    // A client that searches cwd/PATH or honors an arbitrary environment path
    // would run this safe peer. Missing-candidate mode cannot connect to it.
    copy_executable(source, &decoy.join("cedar-agent"))?;
    copy_executable(source, &decoy.join("ssh"))?;
    match case {
        "missing" => {}
        "directory" => fs::create_dir(&agent).map_err(|_| "fixture_directory")?,
        "symlink" => std::os::unix::fs::symlink(decoy.join("cedar-agent"), &agent)
            .map_err(|_| "fixture_symlink")?,
        "dangling_symlink" => std::os::unix::fs::symlink(install.join("absent"), &agent)
            .map_err(|_| "fixture_symlink")?,
        "invalid_elf" => {
            fs::write(&agent, b"controlled invalid executable\n").map_err(|_| "fixture_elf")?;
            fs::set_permissions(&agent, fs::Permissions::from_mode(0o755))
                .map_err(|_| "fixture_mode")?;
        }
        _ => {
            copy_executable(source, &agent)?;
            if case == "nonexecutable" {
                fs::set_permissions(&agent, fs::Permissions::from_mode(0o644))
                    .map_err(|_| "fixture_mode")?;
            }
            if case == "wrong_elf_arch" {
                use std::io::{Seek, SeekFrom, Write};
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .open(&agent)
                    .map_err(|_| "fixture_elf")?;
                file.seek(SeekFrom::Start(18)).map_err(|_| "fixture_elf")?;
                // EM_NONE is invalid on every host and never reaches exec.
                file.write_all(&[0, 0]).map_err(|_| "fixture_elf")?;
            }
        }
    }
    let mut child = Command::new(&probe)
        .args([
            std::ffi::OsStr::new("reject"),
            root.as_os_str(),
            std::ffi::OsStr::new(case),
        ])
        .current_dir(&decoy)
        .env("PATH", &decoy)
        .env("CEDAR_AGENT_BIN", decoy.join("cedar-agent"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "probe_start")?;
    let collected = collect_probe(&mut child);
    if collected.is_err() {
        // This owns only the direct probe child. Failure termination cannot
        // produce a receipt claiming cleanup of that probe's agent child.
        let _ = child.kill();
        let _ = child.wait();
    }
    let (stdout, stderr) = collected?;
    if !stderr.is_empty() {
        return Err("probe_failed");
    }
    verify_receipt(&stdout, case, spawned)?;
    if root.join(".fault-started").exists() != spawned {
        return Err("unexpected_child");
    }
    if spawned && case != "eof" && !root.join(".fault-eof").is_file() {
        return Err("fault_stdin_not_closed");
    }
    scratch.close().map_err(|_| "fixture_removed")
}

#[test]
fn linux_desktop_fixed_sibling_rejects_controlled_failures() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_cedar-client-bundle-probe"));
    let started = Instant::now();
    for &(case, spawned) in CASES {
        if let Err(stage) = verify_case(&source, case, spawned) {
            // Both values come from fixed enumerations, never external errors.
            panic!("linux_desktop_rejection:{case}:{stage}");
        }
    }
    assert!(
        started.elapsed() <= Duration::from_secs(90),
        "linux_desktop_rejection:elapsed_bound"
    );
    println!(
        "\n{}",
        serde_json::json!({
            "kind": "cedar_linux_desktop_rejection_suite",
            "schema_version": 1,
            "status": "success",
            "cases": CASES.len(),
            "pre_spawn_cases": CASES.iter().filter(|(_, spawned)| !spawned).count(),
            "reaped_child_cases": CASES.iter().filter(|(_, spawned)| *spawned).count(),
            "fixed_sibling_only": true,
            "misleading_path_and_cwd_ignored": true,
            "trust_off": true,
            "controlled_fixtures_only": true,
            "fixtures_removed": true,
            "elapsed_ms": started.elapsed().as_millis(),
            "elapsed_bound_ms": 90000,
            "per_probe_bound_ms": PROBE_BOUND.as_millis(),
        })
    );
}

#[test]
fn bounded_probe_drain_observes_but_never_retains_overflow() {
    let mut bytes = vec![b'x'; OUTPUT_BOUND as usize - 1];
    let mut exact = io::Cursor::new(*b"y");
    assert!(!drain(&mut exact, &mut bytes).unwrap());
    assert_eq!(bytes.len(), OUTPUT_BOUND as usize);
    assert!(drain(&mut exact, &mut bytes).unwrap());
    let saved = bytes.clone();
    assert_eq!(
        drain(&mut io::Cursor::new([b'z'; 10]), &mut bytes),
        Err("probe_output_bound_cleanup_unverified")
    );
    assert_eq!(bytes, saved);
}

#[test]
fn bounded_probe_drain_keeps_nonblocking_retries_empty() {
    struct Retry(io::ErrorKind);
    impl Read for Retry {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }
    }
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::Interrupted] {
        let mut bytes = Vec::new();
        assert!(!drain(&mut Retry(kind), &mut bytes).unwrap());
        assert!(bytes.is_empty());
    }
}
