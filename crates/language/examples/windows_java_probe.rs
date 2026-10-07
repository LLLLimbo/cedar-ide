//! Diagnostic-only raw Java launch matrix; this is not an LSP acceptance test.
//!
//! Usage: windows_java_probe JAVA_EXE ASCII_SCRATCH_ROOT UNICODE_DISTRIBUTION_CWD
//! All three paths must already exist and be absolute local-drive paths. The
//! executable must use an ordinary ASCII .exe spelling. Each invocation creates
//! a new ASCII scratch subdirectory and prints one PRIVATE raw JSON report.
//! Redirect stdout/stderr to generated private scratch; never stream or upload
//! them. Only scripts/collect_java_crash.py may produce public diagnostic output.
//! Run only in an isolated host with controlled process creation, as required by
//! cedar_winprocess. No shell, PATH fallback, LSP decoder, or production change.

#[cfg(any(windows, test))]
mod evidence {
    use serde_json::{json, Value};

    pub const CAPTURE_LIMIT: usize = 64 * 1024;
    pub const HEADER_LIMIT: usize = 16 * 1024;
    pub const INPUT: &[u8] = b"Cedar stdin\0\xff\n";
    pub const INPUT_HEX: &str = "436564617220737464696e00ff0a";
    pub const HELLO_SOURCE: &str = r#"import java.util.Arrays;
import java.util.HexFormat;

class CedarOwnedStdioHello {
    public static void main(String[] args) throws Exception {
        System.out.print("CEDAR_JAVA_STDOUT_READY\n");
        System.err.print("CEDAR_JAVA_STDERR_READY\n");
        System.out.flush();
        System.err.flush();
        byte[] expected = HexFormat.of().parseHex("436564617220737464696e00ff0a");
        byte[] actual = System.in.readAllBytes();
        if (!Arrays.equals(actual, expected)) {
            System.err.print("CEDAR_JAVA_STDIN_MISMATCH\n");
            System.exit(7);
        }
        System.out.print("CEDAR_JAVA_STDIN_HEX=" + HexFormat.of().formatHex(actual) + "\n");
        System.out.print("CEDAR_JAVA_STDOUT_DONE\n");
        System.err.print("CEDAR_JAVA_STDERR_DONE\n");
        System.out.flush();
        System.err.flush();
    }
}
"#;

    // The same VM/application options as java_smoke, omitting its -jar and
    // application arguments. Source-file mode uses the JDK's built-in compiler.
    pub const JDT_VM_OPTIONS: &[&str] = &[
        "-Declipse.application=org.eclipse.jdt.ls.core.id1",
        "-Dosgi.bundles.defaultStartLevel=4",
        "-Declipse.product=org.eclipse.jdt.ls.core.product",
        "-Dlog.level=WARNING",
        "-Xmx512m",
        "--add-modules=ALL-SYSTEM",
        "--add-opens",
        "java.base/java.util=ALL-UNNAMED",
        "--add-opens",
        "java.base/java.lang=ALL-UNNAMED",
    ];

    pub fn hex(bytes: &[u8]) -> String {
        const DIGITS: &[u8] = b"0123456789abcdef";
        let mut result = String::with_capacity(bytes.len() * 2);
        for &byte in bytes {
            result.push(DIGITS[usize::from(byte >> 4)] as char);
            result.push(DIGITS[usize::from(byte & 15)] as char);
        }
        result
    }

    #[derive(Default)]
    pub struct Output {
        prefix: Vec<u8>,
        observed: u64,
    }

    impl Output {
        pub fn push(&mut self, bytes: &[u8]) {
            self.observed = self.observed.saturating_add(bytes.len() as u64);
            let retained = bytes.len().min(CAPTURE_LIMIT - self.prefix.len());
            self.prefix.extend_from_slice(&bytes[..retained]);
        }

        pub fn contains(&self, marker: &[u8]) -> bool {
            !marker.is_empty() && self.prefix.windows(marker.len()).any(|part| part == marker)
        }

        pub fn report(&self, eof: bool) -> Value {
            json!({
                "observed_bytes": self.observed,
                "retained_bytes": self.prefix.len(),
                "retention_limit_bytes": CAPTURE_LIMIT,
                "truncated": self.observed > self.prefix.len() as u64,
                "eof_observed": eof,
                "prefix_hex": hex(&self.prefix),
                "prefix_utf8_lossy": String::from_utf8_lossy(&self.prefix),
            })
        }
    }

    // Only initial '#' comment lines and blank lines are retained. Stop before
    // SUMMARY/THREAD/PROCESS sections: hs_err's later sections can contain the
    // environment and other private process data. Never copy the full hs_err.
    pub fn fatal_header(bytes: &[u8]) -> Vec<u8> {
        let mut end = 0;
        for line in bytes[..bytes.len().min(HEADER_LIMIT)].split_inclusive(|b| *b == b'\n') {
            // A partial last line is deliberately omitted.
            if !line.ends_with(b"\n")
                || !(line.starts_with(b"#") || line.iter().all(u8::is_ascii_whitespace))
            {
                break;
            }
            end += line.len();
        }
        bytes[..end].to_vec()
    }
}

#[cfg(windows)]
mod windows_probe {
    use super::evidence::*;
    use cedar_winprocess::{
        LaunchSpec, StdinCancelOutcome, StdinWriteProgress, Stream, WindowsCommand,
        MAX_STDIN_WRITE_BYTES,
    };
    use serde_json::{json, Value};
    use std::{
        ffi::OsString,
        fs,
        io::{self, Read},
        path::{Component, Path, PathBuf, Prefix},
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    const EXECUTION_DEADLINE: Duration = Duration::from_secs(20);
    const CLEANUP_BUDGET: Duration = Duration::from_secs(5);

    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message)
    }

    fn path_text(path: &Path) -> io::Result<&str> {
        path.to_str().ok_or_else(|| invalid("paths must be UTF-8"))
    }

    fn ordinary_local(path: &Path) -> io::Result<PathBuf> {
        let text = path_text(path)?;
        if !path.is_absolute() {
            return Err(invalid("paths must be absolute local-drive paths"));
        }
        match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(_) => Ok(path.to_owned()),
                Prefix::VerbatimDisk(_) => text
                    .strip_prefix(r"\\?\")
                    .map(PathBuf::from)
                    .ok_or_else(|| invalid("unexpected verbatim path spelling")),
                _ => Err(invalid(
                    "UNC/device paths are outside this diagnostic's scope",
                )),
            },
            _ => Err(invalid("expected a local-drive path")),
        }
    }

    fn canonical_ordinary_directory(argument: &OsString, ascii: bool) -> io::Result<PathBuf> {
        let original = Path::new(argument);
        ordinary_local(original)?;
        if !original.is_dir() {
            return Err(invalid(
                "scratch root and distribution cwd must be existing directories",
            ));
        }
        let canonical = original.canonicalize()?;
        let ordinary = ordinary_local(&canonical)?;
        if ordinary.canonicalize()? != canonical {
            return Err(invalid(
                "ordinary cwd spelling changed the selected directory",
            ));
        }
        if path_text(&ordinary)?.is_ascii() != ascii {
            return Err(invalid(if ascii {
                "scratch root must resolve to an ASCII path"
            } else {
                "distribution cwd must resolve to a path containing Unicode"
            }));
        }
        Ok(ordinary)
    }

    fn fatal_headers(directory: &Path) -> io::Result<Vec<Value>> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(pid) = name
                .to_str()
                .and_then(|s| s.strip_prefix("hs_err_pid"))
                .and_then(|s| s.strip_suffix(".log"))
            else {
                continue;
            };
            if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            if entry.file_type()?.is_file() {
                paths.push(entry.path());
                if paths.len() == 8 {
                    break;
                }
            }
        }
        paths.sort();
        let mut reports = Vec::new();
        for path in paths {
            let file = fs::File::open(&path)?;
            let size = file.metadata()?.len();
            let mut prefix = Vec::new();
            file.take(HEADER_LIMIT as u64).read_to_end(&mut prefix)?;
            let header = fatal_header(&prefix);
            reports.push(json!({
                "file_name": path.file_name().and_then(|name| name.to_str()),
                "file_size_bytes": size,
                "header_hex": hex(&header),
                "header_utf8_lossy": String::from_utf8_lossy(&header),
                "header_limit_bytes": HEADER_LIMIT,
                "remaining_file_intentionally_omitted": size > header.len() as u64,
            }));
        }
        Ok(reports)
    }

    #[derive(Default)]
    struct Input {
        accepted: usize,
        pending: bool,
        closed: bool,
    }

    impl Input {
        fn pump(&mut self, command: &mut WindowsCommand, bytes: &[u8]) -> io::Result<()> {
            if self.closed {
                return Ok(());
            }
            if self.accepted == bytes.len() && !self.pending {
                command.close_stdin()?;
                self.closed = true;
                return Ok(());
            }
            let progress = if self.pending {
                command.poll_stdin_write()?
            } else {
                let end = (self.accepted + MAX_STDIN_WRITE_BYTES).min(bytes.len());
                command.begin_stdin_write(&bytes[self.accepted..end])?
            };
            self.pending = progress == StdinWriteProgress::Pending;
            match progress {
                StdinWriteProgress::Written(n) if n > 0 && n <= bytes.len() - self.accepted => {
                    self.accepted += n;
                }
                StdinWriteProgress::Pending => {}
                _ => {
                    return Err(io::Error::other(format!(
                        "unexpected stdin progress: {progress:?}"
                    )))
                }
            }
            Ok(())
        }
    }

    fn run_case(
        run_dir: &Path,
        name: &str,
        executable: &Path,
        canonical_executable: &Path,
        cwd: &Path,
        hello: bool,
    ) -> Value {
        let started = Instant::now();
        let mut report = json!({
            "name": name, "diagnostic_only": true,
            "executable": executable, "cwd": cwd,
            "execution_deadline_ms": EXECUTION_DEADLINE.as_millis(),
            "cleanup_poll_budget_ms": CLEANUP_BUDGET.as_millis(),
            "exit_code": null, "exit_code_hex": null,
            "job_active_processes_final": null, "job_zero_observed": false,
            "cleanup_verified": false,
        });
        let directory = run_dir.join(name);
        let errors_directory = directory.join("errors");
        let setup = (|| -> io::Result<LaunchSpec> {
            // Recheck identity immediately before each launch; this never
            // substitutes another Java installation for the selected one.
            if executable.canonicalize()? != canonical_executable {
                return Err(invalid(
                    "executable no longer resolves to the selected canonical file",
                ));
            }
            fs::create_dir(&directory)?;
            fs::create_dir(&errors_directory)?;
            let error_file = errors_directory.join("hs_err_pid%p.log");
            // OpenJDK 21 product flag and Windows implementation:
            // https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/runtime/globals.hpp
            // https://github.com/openjdk/jdk21u/blob/master/src/hotspot/os/windows/os_windows.cpp
            let mut arguments = vec![
                format!("-XX:ErrorFile={}", path_text(&error_file)?),
                "-XX:-CreateCoredumpOnCrash".into(),
            ];
            if hello {
                arguments.extend(JDT_VM_OPTIONS.iter().map(|s| (*s).to_owned()));
                // Override inherited CLASSPATH with this newly created ASCII
                // case directory and pin the source-language level to Java 21.
                arguments.extend([
                    "--class-path".into(),
                    path_text(&directory)?.to_owned(),
                    "--source".into(),
                    "21".into(),
                ]);
                let source = directory.join("CedarOwnedStdioHello.java");
                fs::write(&source, HELLO_SOURCE)?;
                arguments.push(path_text(&source)?.to_owned());
            } else {
                arguments.push("-version".into());
            }
            if !arguments.iter().all(|argument| argument.is_ascii()) {
                return Err(invalid("all Java argv entries must be ASCII"));
            }
            report["arguments"] = json!(arguments);
            report["error_file_pattern"] = json!(error_file);
            report["canonical_executable_matches"] = json!(true);
            Ok(LaunchSpec {
                executable: executable.to_owned(),
                arguments,
                cwd: cwd.to_owned(),
            })
        })();
        let spec = match setup {
            Ok(spec) => spec,
            Err(error) => {
                report["outcome"] = json!("setup_error");
                report["errors"] = json!([format!("prepare case: {error}")]);
                return report;
            }
        };
        let mut command = match WindowsCommand::spawn_suspended_with_piped_stdin(&spec) {
            Ok(command) => command,
            Err(error) => {
                report["outcome"] = json!("spawn_error");
                report["errors"] = json!([format!("spawn owned piped-stdin child: {error}")]);
                return report;
            }
        };
        report["process_id"] = json!(command.process_id());
        let mut errors = Vec::new();
        let mut stdout = Output::default();
        let mut stderr = Output::default();
        let mut input = Input::default();
        let bytes = if hello { INPUT } else { &[] };
        let mut exit = None;
        let mut stdout_eof = false;
        let mut stderr_eof = false;
        let mut capture_failed = false;
        let mut timed_out = false;
        let mut resumed = false;
        match command.resume() {
            Ok(()) => resumed = true,
            Err(error) => errors.push(format!("resume: {error}")),
        }
        let deadline = started + EXECUTION_DEADLINE;
        if resumed {
            loop {
                match command.capture_round(|stream, chunk| match stream {
                    Stream::Stdout => stdout.push(chunk),
                    Stream::Stderr => stderr.push(chunk),
                }) {
                    Ok(progress) => {
                        stdout_eof |= progress.stdout_eof;
                        stderr_eof |= progress.stderr_eof;
                    }
                    Err(error) => {
                        capture_failed = true;
                        errors.push(format!("capture: {error}"));
                        break;
                    }
                }
                match command.try_exit() {
                    Ok(Some(code)) => {
                        exit = Some(code);
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        errors.push(format!("observe exit: {error}"));
                        break;
                    }
                }
                if Instant::now() >= deadline {
                    timed_out = true;
                    break;
                }
                if let Err(error) = input.pump(&mut command, bytes) {
                    errors.push(format!("stdin: {error}"));
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        report["root_exit_observed_before_termination"] = json!(exit.is_some());
        report["execution_elapsed_ms"] = json!(started.elapsed().as_millis());
        // Root exit alone is not tree cleanup. Always terminate this exact Job,
        // then observe both its root handle and ActiveProcesses == 0. No PID
        // lookup, process-name kill, detached reader, or abandoned pending I/O.
        if let Err(error) = command.terminate_tree() {
            errors.push(format!("terminate owned Job: {error}"));
        }
        let stdin_completed = match command.cancel_stdin_and_complete() {
            Ok(outcome) => {
                if let StdinCancelOutcome::Written(n) = outcome {
                    input.accepted += n;
                }
                report["stdin_cancel_outcome"] = json!(format!("{outcome:?}"));
                true
            }
            Err(error) => {
                errors.push(format!("complete stdin cancellation: {error}"));
                false
            }
        };
        let cleanup_deadline = Instant::now() + CLEANUP_BUDGET;
        let mut final_active = None;
        let mut exit_query_failed = false;
        let mut job_query_failed = false;
        loop {
            if !capture_failed && !(stdout_eof && stderr_eof) {
                match command.capture_round(|stream, chunk| match stream {
                    Stream::Stdout => stdout.push(chunk),
                    Stream::Stderr => stderr.push(chunk),
                }) {
                    Ok(progress) => {
                        stdout_eof |= progress.stdout_eof;
                        stderr_eof |= progress.stderr_eof;
                    }
                    Err(error) => {
                        capture_failed = true;
                        errors.push(format!("final capture: {error}"));
                    }
                }
            }
            if exit.is_none() && !exit_query_failed {
                match command.try_exit() {
                    Ok(code) => exit = code,
                    Err(error) => {
                        exit_query_failed = true;
                        errors.push(format!("final exit query: {error}"));
                    }
                }
            }
            if !job_query_failed {
                match command.active_processes() {
                    Ok(count) => final_active = Some(count),
                    Err(error) => {
                        job_query_failed = true;
                        errors.push(format!("Job accounting: {error}"));
                    }
                }
            }
            if exit.is_some()
                && final_active == Some(0)
                && (capture_failed || (stdout_eof && stderr_eof))
            {
                break;
            }
            if Instant::now() >= cleanup_deadline {
                errors
                    .push("cleanup polling budget exceeded; owned Drop still joins cleanup".into());
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        let capture_completed = match command.cancel_capture_and_complete() {
            Ok(()) => true,
            Err(error) => {
                errors.push(format!("complete capture cancellation: {error}"));
                false
            }
        };
        // wait_exit is only called after try_exit observed the owned handle
        // signaled. Drop remains the fail-closed backstop for exceptional kernel
        // failures; cancellation joining may exceed the execution deadline.
        let root_joined = if let Some(observed) = exit {
            match command.wait_exit() {
                Ok(joined) if joined == observed => true,
                Ok(_) => {
                    errors.push("root exit changed while joining".into());
                    false
                }
                Err(error) => {
                    errors.push(format!("join root: {error}"));
                    false
                }
            }
        } else {
            false
        };
        report["job_active_processes_final"] = json!(final_active);
        report["job_zero_observed"] = json!(final_active == Some(0));
        report["root_joined"] = json!(root_joined);
        report["capture_cancellation_completed"] = json!(capture_completed);
        report["stdin_cancellation_completed"] = json!(stdin_completed);
        report["cleanup_verified"] =
            json!(root_joined && final_active == Some(0) && capture_completed && stdin_completed);
        if let Some(exit) = exit {
            report["exit_code"] = json!(exit.code);
            report["exit_code_hex"] = json!(format!("0x{:08X}", exit.code));
        }
        report["timed_out"] = json!(timed_out);
        report["stdin_expected_bytes"] = json!(bytes.len());
        report["stdin_accepted_bytes"] = json!(input.accepted);
        report["stdin_eof_sent_before_cleanup"] = json!(input.closed);
        report["stdout"] = stdout.report(stdout_eof);
        report["stderr"] = stderr.report(stderr_eof);
        if hello {
            report["hello_markers"] = json!({
                "stdout_ready": stdout.contains(b"CEDAR_JAVA_STDOUT_READY\n"),
                "stderr_ready": stderr.contains(b"CEDAR_JAVA_STDERR_READY\n"),
                "stdin_echo": stdout.contains(format!("CEDAR_JAVA_STDIN_HEX={INPUT_HEX}\n").as_bytes()),
                "stdout_done": stdout.contains(b"CEDAR_JAVA_STDOUT_DONE\n"),
                "stderr_done": stderr.contains(b"CEDAR_JAVA_STDERR_DONE\n"),
            });
        }
        // Finish all owned cleanup before reading any diagnostic files or
        // attempting the next matrix case. Never treat Drop as proof of zero.
        drop(command);
        match fatal_headers(&errors_directory) {
            Ok(headers) => report["fatal_error_headers"] = json!(headers),
            Err(error) => errors.push(format!("read bounded fatal headers: {error}")),
        }
        report["outcome"] = json!(if timed_out {
            "execution_deadline_exceeded"
        } else if !errors.is_empty() {
            "supervision_error"
        } else if exit.is_some_and(|exit| exit.code != 0) {
            "child_nonzero_exit"
        } else {
            "child_exited"
        });
        report["errors"] = json!(errors);
        report["total_elapsed_ms"] = json!(started.elapsed().as_millis());
        report
    }

    pub fn run() -> io::Result<Value> {
        let arguments: Vec<_> = std::env::args_os().skip(1).collect();
        if arguments.len() != 3 {
            return Err(invalid(
                "usage: windows_java_probe JAVA_EXE ASCII_SCRATCH_ROOT UNICODE_DISTRIBUTION_CWD",
            ));
        }
        let executable = PathBuf::from(&arguments[0]);
        if ordinary_local(&executable)? != executable
            || !path_text(&executable)?.is_ascii()
            || !executable
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
            || !executable.is_file()
        {
            return Err(invalid(
                "JAVA_EXE must be an existing absolute ordinary ASCII .exe path",
            ));
        }
        let canonical = executable.canonicalize()?;
        if !path_text(&canonical)?.is_ascii()
            || !matches!(canonical.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::VerbatimDisk(_)))
        {
            return Err(invalid(
                "JAVA_EXE must canonicalize to an ASCII verbatim local-drive path",
            ));
        }
        let scratch = canonical_ordinary_directory(&arguments[1], true)?;
        let unicode_cwd = canonical_ordinary_directory(&arguments[2], false)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let run_dir = scratch.join(format!(
            "windows-java-probe-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&run_dir)?;
        let matrix = [
            ("version_ordinary_ascii_cwd", &executable, &run_dir, false),
            ("version_canonical_ascii_cwd", &canonical, &run_dir, false),
            (
                "version_ordinary_unicode_cwd",
                &executable,
                &unicode_cwd,
                false,
            ),
            (
                "version_canonical_unicode_cwd",
                &canonical,
                &unicode_cwd,
                false,
            ),
            (
                "hello_ordinary_unicode_cwd",
                &executable,
                &unicode_cwd,
                true,
            ),
            (
                "hello_canonical_unicode_cwd",
                &canonical,
                &unicode_cwd,
                true,
            ),
        ];
        // No early return on a case failure: comparing failures is the purpose.
        let cases: Vec<_> = matrix
            .into_iter()
            .map(|(name, exe, cwd, hello)| run_case(&run_dir, name, exe, &canonical, cwd, hello))
            .collect();
        Ok(json!({
            "schema_version": 1,
            "driver_status": "diagnostic_only",
            "driver_completed": true,
            "acceptance_claimed": false,
            "case_count": cases.len(),
            "scratch_directory": run_dir,
            "canonical_executable": canonical,
            "stdout_stderr_retention_limit_per_stream": CAPTURE_LIMIT,
            "fatal_file_policy": "Only initial bounded comment headers are included. Full hs_err files may contain environment/process data and must not be published. No minidump is read or included.",
            "deadline_policy": "20-second execution deadline per case; bounded final drain, then mandatory owned cleanup joins. Exceptional kernel cleanup may exceed the deadline.",
            "cases": cases,
        }))
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    let (report, code) = match windows_probe::run() {
        Ok(report) => (report, std::process::ExitCode::SUCCESS),
        Err(error) => (
            serde_json::json!({"driver_status": "setup_error", "diagnostic_only": true, "driver_completed": false, "acceptance_claimed": false, "error": error.to_string()}),
            std::process::ExitCode::from(2),
        ),
    };
    #[cfg(not(windows))]
    let (report, code) = (
        serde_json::json!({"driver_status": "unsupported_platform", "diagnostic_only": true, "driver_completed": false, "acceptance_claimed": false, "error": "windows_java_probe requires native Windows; no Java process was launched"}),
        std::process::ExitCode::from(2),
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serialize diagnostic report")
    );
    code
}

#[cfg(test)]
mod tests {
    use super::evidence::*;

    #[test]
    fn retains_exact_bounded_prefix_and_counts_discarded_bytes() {
        let mut output = Output::default();
        output.push(&[0, 255, b'\n']);
        output.push(&vec![b'x'; CAPTURE_LIMIT]);
        output.push(b"discarded");
        let report = output.report(false);
        assert_eq!(report["retained_bytes"], CAPTURE_LIMIT);
        assert_eq!(report["observed_bytes"], CAPTURE_LIMIT + 12);
        assert_eq!(report["truncated"], true);
        assert!(report["prefix_hex"].as_str().unwrap().starts_with("00ff0a"));
        assert!(output.contains(&[0, 255, b'\n']));
        assert!(!output.contains(b"discarded"));
        assert!(!output.contains(b""));
    }

    #[test]
    fn keeps_hello_source_and_vm_arguments_ascii() {
        assert!(HELLO_SOURCE.is_ascii());
        assert!(JDT_VM_OPTIONS.iter().all(|argument| argument.is_ascii()));
        assert_eq!(hex(INPUT), INPUT_HEX);
        assert!(HELLO_SOURCE.contains(INPUT_HEX));
        assert!(HELLO_SOURCE.contains("readAllBytes()"));
        assert!(JDT_VM_OPTIONS.contains(&"-Xmx512m"));
    }

    #[test]
    fn fatal_header_stops_before_process_or_environment_sections() {
        let header = b"# A fatal error has been detected\r\n# EXCEPTION_ACCESS_VIOLATION\r\n\r\n";
        let mut file = header.to_vec();
        file.extend_from_slice(b"---------------  S U M M A R Y ------------\nCommand Line: secret\nEnvironment Variables:\nTOKEN=private\n");
        assert_eq!(fatal_header(&file), header);
        assert_eq!(
            fatal_header(b"Environment Variables:\nTOKEN=private\n"),
            b""
        );
        assert_eq!(fatal_header(b"# complete\n# incomplete"), b"# complete\n");
        assert!(fatal_header(&vec![b'#'; HEADER_LIMIT + 10]).is_empty());
    }
}
