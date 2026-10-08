//! Opt-in real-server test using an installed Eclipse JDT LS distribution.
//! Usage: cargo run -p cedar-language --example java_smoke -- /path/to/jdtls [/path/to/java] [--resolve-imports]
//! Only a synthetic, temporary Eclipse Java project is created and inspected.
//! Windows requires an explicit absolute java.exe. See docs/WINDOWS_JAVA_ACCEPTANCE.md.
use cedar_language::{
    ClientOptions, LspClient, LspEvent, Position, ProcessConfig, PublishDiagnostics, Range,
};
use serde_json::{json, Value};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SOURCE: &str = "public class Main {\n    public static void main(String[] args) {\n        String greeting = \"Hello Cedar\";\n        System.out.println(greeting);\n        int broken = \"oops\";\n    }\n}\n";

type SmokeResult<T> = Result<T, Box<dyn Error>>;

fn corrected_source() -> String {
    SOURCE.replace("int broken = \"oops\";", "int correctedOnly = 42;")
}

fn marker_range(source: &str, marker: &str) -> Range {
    let offset = source.find(marker).expect("known fixture marker");
    Range {
        start: Position::end_of(&source[..offset]),
        end: Position::end_of(&source[..offset + marker.len()]),
    }
}

fn hover_contains(hover: &Value, expected: &[&str]) -> bool {
    fn text(value: &Value) -> String {
        match value {
            Value::String(value) => value.clone(),
            Value::Array(values) => values.iter().map(text).collect::<Vec<_>>().join("\n"),
            Value::Object(value) => value
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            _ => String::new(),
        }
    }
    let contents = text(&hover["contents"]);
    !contents.trim().is_empty()
        && expected.iter().all(|part| {
            contents
                .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '$')
                .any(|word| word == *part)
        })
}

fn exact_definition(definition: &Value, uri: &str, range: Range) -> bool {
    let location = match definition.as_array() {
        Some(locations) if locations.len() == 1 => &locations[0],
        Some(_) => return false,
        None => definition,
    };
    let expected = json!(range);
    (location["uri"]
        .as_str()
        .is_some_and(|actual| same_local_uri(actual, uri))
        && location["range"] == expected)
        || (location["targetUri"]
            .as_str()
            .is_some_and(|actual| same_local_uri(actual, uri))
            && location["targetSelectionRange"] == expected)
}

fn same_local_uri(actual: &str, expected: &str) -> bool {
    fn local_path(uri: &str) -> Option<Vec<u8>> {
        let path = uri.strip_prefix("file:")?;
        // Accept file:/absolute and file:///absolute, never a remote authority
        // or UNC path. JDT may return raw Unicode instead of percent escapes.
        let path = path.strip_prefix("//").unwrap_or(path);
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#', '\\']) {
            return None;
        }
        let mut decoded = Vec::with_capacity(path.len());
        let mut bytes = path.bytes();
        while let Some(byte) = bytes.next() {
            decoded.push(if byte == b'%' {
                let high = (bytes.next()? as char).to_digit(16)?;
                let low = (bytes.next()? as char).to_digit(16)?;
                ((high << 4) | low) as u8
            } else {
                byte
            });
        }
        if decoded.contains(&0) {
            return None;
        }
        #[cfg(windows)]
        if decoded.get(2) == Some(&b':') && decoded.get(1).is_some_and(u8::is_ascii_alphabetic) {
            decoded[1].make_ascii_uppercase();
        }
        Some(decoded)
    }
    match (local_path(actual), local_path(expected)) {
        (Some(actual), Some(expected)) => actual == expected,
        _ => false,
    }
}

fn diagnostics_match(d: &PublishDiagnostics, uri: &str, version: i32, expect_error: bool) -> bool {
    if !same_local_uri(&d.uri, uri) || d.version.is_some_and(|v| v != version) {
        return false;
    }
    if expect_error {
        d.diagnostics.iter().any(|d| {
            d.severity == Some(1)
                && d.message.contains("cannot convert from String to int")
                && d.range == marker_range(SOURCE, "\"oops\"")
        })
    } else {
        // JDT can omit the document version. An arbitrary empty notification
        // could be old; require a diagnostic that only this corrected draft can
        // produce. The synthetic project's unused-local severity is explicit.
        !d.diagnostics.iter().any(|d| d.severity == Some(1))
            && d.diagnostics.iter().any(|d| {
                d.severity == Some(2)
                    && d.message.contains("correctedOnly")
                    && d.message.contains("not used")
                    && d.range == marker_range(&corrected_source(), "correctedOnly")
            })
    }
}

fn assert_source_unchanged(source: &Path, phase: &str) -> SmokeResult<()> {
    let unchanged = std::fs::read(source)? == SOURCE.as_bytes();
    println!(
        "{}",
        json!({"kind":"source_bytes","phase":phase,"unchanged":unchanged})
    );
    if !unchanged {
        return Err(format!("source bytes changed during {phase}").into());
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn require_graceful_exit(code: u32) -> SmokeResult<()> {
    if code != 0 {
        return Err(format!(
            "Java root exited with code {code}; cleanup alone is not graceful shutdown"
        )
        .into());
    }
    Ok(())
}

fn java_executable(argument: Option<&std::ffi::OsString>) -> SmokeResult<PathBuf> {
    #[cfg(windows)]
    {
        let path = Path::new(argument.ok_or("Windows requires an explicit absolute java.exe")?);
        if !path.is_absolute()
            || !path.is_file()
            || !path.file_name().is_some_and(|name| {
                name.to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("java.exe"))
            })
        {
            return Err(
                "Windows requires an existing absolute native java.exe, without PATH fallback"
                    .into(),
            );
        }
        let executable = java_local_path(path)?;
        if !executable.to_str().is_some_and(str::is_ascii) {
            return Err("Windows Java acceptance requires an ASCII JDK installation path".into());
        }
        Ok(executable)
    }
    #[cfg(not(windows))]
    Ok(argument.map(PathBuf::from).unwrap_or_else(|| "java".into()))
}

// Java 21's java.io parser treats Rust's \\?\ canonical prefix as UNC-like.
// The native 0.8.8 matrix also isolates JVM startup crashes to the verbatim
// executable spelling. Use an identity-checked ordinary local-drive spelling
// only in this Java launch recipe. Generic WindowsCommand remains literal.
// This probe does not cover UNC/device paths.
#[cfg(windows)]
fn ordinary_windows_local_path(path: &Path) -> SmokeResult<PathBuf> {
    use std::path::{Component, Prefix};
    let text = path.to_str().ok_or("Java fixture paths must be UTF-8")?;
    let plain = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(_) => text
                .strip_prefix(r"\\?\")
                .ok_or("unexpected verbatim local-drive path")?,
            Prefix::Disk(_) if path.is_absolute() => text,
            _ => {
                return Err(
                    "Java acceptance requires local-drive paths, not UNC/device paths".into(),
                )
            }
        },
        _ => return Err("Java acceptance requires an absolute local-drive path".into()),
    };
    Ok(PathBuf::from(plain))
}

fn java_local_path(path: &Path) -> SmokeResult<PathBuf> {
    #[cfg(windows)]
    {
        let canonical = path.canonicalize()?;
        let plain = ordinary_windows_local_path(&canonical)?;
        // Ordinary Windows parsing may normalize trailing dots/spaces or device
        // names differently. Never silently change the selected file/directory.
        if plain.canonicalize()? != canonical {
            return Err("ordinary Java path does not resolve to the same canonical fixture".into());
        }
        Ok(plain)
    }
    #[cfg(not(windows))]
    Ok(path.to_path_buf())
}

// Java's Windows native launcher converts argv through the system code page.
// Its Unicode cwd is handled separately. Keep the selected jar physically under
// that cwd, but pass only an exact ASCII relative name to the native launcher.
fn relative_launcher(directory: &Path, launcher: &Path) -> SmokeResult<PathBuf> {
    let relative = launcher.strip_prefix(directory)?;
    if relative.as_os_str().is_empty()
        || !relative.to_str().is_some_and(str::is_ascii)
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || directory.join(relative).canonicalize()? != launcher.canonicalize()?
    {
        return Err(
            "Equinox launcher must have an exact ASCII relative path inside its distribution"
                .into(),
        );
    }
    Ok(relative.to_path_buf())
}

// Diagnostic-only opt-in from the isolated CI parent. Never use a Unicode
// native ErrorFile argument or upload a minidump/raw process-memory report.
fn crash_report_arguments(directory: &Path) -> SmokeResult<Vec<std::ffi::OsString>> {
    if !directory.is_absolute() || !directory.is_dir() {
        return Err("CEDAR_JAVA_ERROR_DIR must name an existing absolute directory".into());
    }
    let directory = java_local_path(directory)?;
    let text = directory.to_str().ok_or("crash directory must be ASCII")?;
    if !text.is_ascii()
        || text
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'%')
    {
        return Err(
            "crash directory must be ASCII without control characters or template escapes".into(),
        );
    }
    Ok(vec![
        format!(
            "-XX:ErrorFile={}",
            directory.join("hs_err_pid%p.log").display()
        )
        .into(),
        "-XX:-CreateCoredumpOnCrash".into(),
    ])
}

// ProcessConfig inherits its parent's environment. Fail rather than alter a
// possibly multithreaded process or allow launcher/socket environment injection.
fn check_launch_environment() -> SmokeResult<()> {
    for name in [
        "CLIENT_PORT",
        "CLIENT_HOST",
        "socket.stream.debug",
        "JDK_JAVA_OPTIONS",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
    ] {
        if std::env::var_os(name).is_some() {
            return Err(
                format!("unset {name} in the test parent before starting java_smoke").into(),
            );
        }
    }
    Ok(())
}

#[cfg(windows)]
mod process_observer {
    use super::SmokeResult;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    pub struct ObservedProcess {
        handle: OwnedHandle,
        pub creation_time: u64,
    }

    impl ObservedProcess {
        pub fn open(pid: u32) -> SmokeResult<Self> {
            // SAFETY: This PID belongs to the client just launched. Request only
            // noninheritable query/wait access, never termination authority.
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            if raw.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: OpenProcess returned a fresh owned handle; close once.
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            let mut times = [FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            }; 4];
            // SAFETY: Handle has query access and all four output pointers refer
            // to distinct valid FILETIME objects for the duration of the call.
            let success = unsafe {
                GetProcessTimes(
                    handle.as_raw_handle(),
                    &mut times[0],
                    &mut times[1],
                    &mut times[2],
                    &mut times[3],
                )
            };
            if success == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let observed = Self {
                handle,
                creation_time: (u64::from(times[0].dwHighDateTime) << 32)
                    | u64::from(times[0].dwLowDateTime),
            };
            observed.assert_live()?;
            Ok(observed)
        }

        pub fn assert_live(&self) -> SmokeResult<()> {
            // SAFETY: Held SYNCHRONIZE handle, zero-time observation only.
            match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
                WAIT_TIMEOUT => Ok(()),
                WAIT_OBJECT_0 => Err("observed Java root exited before semantic acceptance".into()),
                _ => Err(std::io::Error::last_os_error().into()),
            }
        }

        pub fn exit_code(&self) -> SmokeResult<u32> {
            // SAFETY: This same observation handle remains owned through the
            // bounded wait, so PID reuse cannot turn a dead process into a live one.
            match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 1500) } {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => {
                    return Err("observed Java root remains live after shutdown/Drop".into())
                }
                _ => return Err(std::io::Error::last_os_error().into()),
            }
            let mut code = 0;
            // SAFETY: Valid query handle and writable DWORD. Check termination
            // by wait first; 259 is a valid exit code, not a liveness assertion.
            if unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(code)
        }
    }
}

// /proc's status counters use KiB even though the text suffix is "kB".
// RSS is a kernel accounting snapshot, not PSS or a heap-use measurement.
#[cfg(any(target_os = "linux", test))]
fn status_kib(status: &str, field: &str) -> Result<u64, std::io::Error> {
    let value = status
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key == field).then_some(value)
        })
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("missing {field} in process status"),
            )
        })?;
    let mut words = value.split_whitespace();
    let number = words.next().and_then(|value| value.parse::<u64>().ok());
    if words.next() != Some("kB") || words.next().is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid {field} units in process status"),
        ));
    }
    number.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid {field} value in process status"),
        )
    })
}

fn sample_jvm_memory(pid: u32, phase: &str, start: Instant) -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "linux")]
    {
        let source = format!("/proc/{pid}/status");
        let status = std::fs::read_to_string(&source)?;
        let rss = status_kib(&status, "VmRSS")?;
        let hwm = status_kib(&status, "VmHWM")?;
        println!(
            "{}",
            json!({
                "kind":"jvm_memory", "phase":phase, "elapsed_ms":start.elapsed().as_millis(),
                "pid":pid, "source":source, "scope":"direct_language_server_jvm_only",
                "vm_rss_kib":rss, "vm_hwm_kib":hwm,
                "vm_rss_mib":rss as f64 / 1024.0, "vm_hwm_mib":hwm as f64 / 1024.0
            })
        );
    }
    #[cfg(not(target_os = "linux"))]
    println!(
        "{}",
        json!({
            "kind":"jvm_memory_unavailable", "phase":phase, "pid":pid,
            "elapsed_ms":start.elapsed().as_millis(), "reason":"/proc status measurement is Linux-only"
        })
    );
    Ok(())
}

fn file_uri(path: &Path) -> Result<String, Box<dyn Error>> {
    let path = path.canonicalize()?;
    let text = path
        .to_str()
        .ok_or("fixture path must be UTF-8")?
        .replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    if text.starts_with("//") || text.starts_with("UNC/") {
        return Err("this local-drive acceptance does not support UNC fixture paths".into());
    }
    let mut uri = if text.starts_with('/') {
        "file://".to_owned()
    } else {
        "file:///".to_owned()
    };
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(uri)
}

fn await_diagnostics(
    client: &LspClient,
    uri: &str,
    version: i32,
    expect_error: bool,
) -> Result<PublishDiagnostics, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        match client.next_event(Duration::from_millis(500))? {
            Some(LspEvent::Diagnostics(d)) => {
                println!("{}", json!({"kind":"diagnostics","payload":d}));
                if diagnostics_match(&d, uri, version, expect_error) {
                    return Ok(d);
                }
            }
            Some(LspEvent::Closed(error)) => return Err(error.into()),
            Some(LspEvent::Lagged { dropped }) => {
                return Err(format!("lost {dropped} events during smoke test").into())
            }
            Some(LspEvent::UnsupportedServerRequest { method, .. }) => {
                println!("{}", json!({"kind":"unsupported_callback","method":method}))
            }
            Some(LspEvent::Notification { method, params }) => println!(
                "{}",
                json!({"kind":"notification","method":method,"params":params})
            ),
            _ => {}
        }
    }
    Err(format!(
        "timed out awaiting diagnostics for version {version}, expect_error={expect_error}"
    )
    .into())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let resolve_imports = args.last().is_some_and(|arg| arg == "--resolve-imports");
    if resolve_imports {
        args.pop();
    }
    if args.is_empty() || args.len() > 2 {
        eprintln!("Usage: java_smoke JDTLS_DIRECTORY [JAVA_EXECUTABLE] [--resolve-imports]");
        std::process::exit(2);
    }
    let java = java_executable(args.get(1))?;
    check_launch_environment()?;
    let jdtls = Path::new(&args[0]).canonicalize()?;
    let jars: Vec<_> = std::fs::read_dir(jdtls.join("plugins"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("org.eclipse.equinox.launcher_")
            }) && path.extension().is_some_and(|ext| ext == "jar")
        })
        .collect();
    if jars.len() != 1 {
        return Err("expected one exact Equinox launcher JAR in JDT LS plugins".into());
    }
    let temp = tempfile::Builder::new()
        .prefix("cedar java smoke 雪 ")
        .tempdir()?;
    let project = temp.path().join("workspace with spaces 雪");
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::create_dir_all(project.join(".settings"))?;
    std::fs::write(project.join(".project"), "<?xml version=\"1.0\"?><projectDescription><name>cedar-language-smoke</name><projects/><buildSpec><buildCommand><name>org.eclipse.jdt.core.javabuilder</name><arguments/></buildCommand></buildSpec><natures><nature>org.eclipse.jdt.core.javanature</nature></natures></projectDescription>")?;
    std::fs::write(project.join(".classpath"), "<?xml version=\"1.0\"?><classpath><classpathentry kind=\"src\" path=\"src\"/><classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/><classpathentry kind=\"output\" path=\"bin\"/></classpath>")?;
    std::fs::write(project.join(".settings/org.eclipse.jdt.core.prefs"), "eclipse.preferences.version=1\norg.eclipse.jdt.core.compiler.codegen.targetPlatform=21\norg.eclipse.jdt.core.compiler.compliance=21\norg.eclipse.jdt.core.compiler.source=21\norg.eclipse.jdt.core.compiler.problem.unusedLocal=warning\n")?;
    let source = project.join("src/Main.java");
    std::fs::write(&source, SOURCE)?;
    let root_uri = file_uri(&project)?;
    let document_uri = file_uri(&source)?;
    let mut config = ProcessConfig::new(java);
    config.working_directory = Some(java_local_path(&jdtls)?);
    config.args = [
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
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    if let Some(directory) = std::env::var_os("CEDAR_JAVA_ERROR_DIR") {
        config
            .args
            .extend(crash_report_arguments(Path::new(&directory))?);
    }
    config.args.push("-jar".into());
    config
        .args
        .push(relative_launcher(&jdtls, &jars[0])?.into());
    config.args.push("-configuration".into());
    let platform = if cfg!(target_os = "macos") {
        "config_mac"
    } else if cfg!(target_os = "windows") {
        "config_win"
    } else {
        "config_linux"
    };
    let configuration = jdtls.join(platform);
    if !configuration.is_dir() {
        return Err(format!("missing JDT LS {platform} directory").into());
    }
    config.args.push(file_uri(&configuration)?.into());
    config.args.push("-data".into());
    let options = ClientOptions {
        request_timeout: Duration::from_secs(60),
        shutdown_timeout: Duration::from_secs(if cfg!(windows) { 10 } else { 3 }),
        ..ClientOptions::default()
    };
    println!(
        "{}",
        json!({
            "kind":"run_metadata", "fixture":"fresh synthetic Eclipse Java project, one source file",
            "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
            "jvm_max_heap_mib":512, "memory_scope":"direct_language_server_jvm_only",
            "frontend_or_agent_included":false, "initial_project_and_server_data_fresh":true,
            "java_executable":config.program, "jdtls_directory":jdtls,
            "launcher":jars[0], "configuration":configuration,
            "working_directory":config.working_directory,
            "launch_representation":"verified ordinary Java executable; Unicode distribution cwd; ASCII-relative jar; encoded location URLs",
            "literal_launch_arguments_before_data":config.args,
            "shutdown_grace_secs":options.shutdown_timeout.as_secs(),
            "document_uri":document_uri, "sessions":3, "resolve_imports":resolve_imports
        })
    );
    let start = Instant::now();
    let mut previous_identity = None;
    let sessions = [
        ("initial", "jdt data initial 雪"),
        ("restart_fresh_data", "jdt data restart 雪"),
        ("restart_same_data", "jdt data restart 雪"),
    ];
    let result = (|| -> SmokeResult<()> {
        for (session, data) in sessions {
            let data = temp.path().join(data);
            let fresh = !data.exists();
            std::fs::create_dir_all(&data)?;
            let mut session_config = config.clone();
            session_config.args.push(file_uri(&data)?.into());
            println!(
                "{}",
                json!({"kind":"session_start","session":session,
                "data_directory":data,"fresh_data":fresh,"previous_client_dropped":previous_identity.is_some()})
            );
            previous_identity = Some(run_session(
                session_config,
                options.clone(),
                &root_uri,
                &document_uri,
                &source,
                &data,
                resolve_imports,
                session,
                previous_identity,
            )?);
        }
        Ok(())
    })();
    let disk = assert_source_unchanged(&source, "after_all_clients_dropped");
    let cleanup = temp.close();
    println!(
        "{}",
        json!({"kind":"fixture_cleanup","removed":cleanup.is_ok(),
        "source_unchanged_before_removal":disk.is_ok(),"sessions_succeeded":result.is_ok()})
    );
    result?;
    disk?;
    cleanup?;
    println!(
        "{}",
        json!({"kind":"pass","elapsed_ms":start.elapsed().as_millis(),
        "sessions":3,"fresh_data_restart":true,"same_data_restart":true,
        "source_bytes_unchanged":true,"fixture_removed":true,"lazy_import_resolve_checked":resolve_imports,
        "frontend_or_agent_included":false,"windows_full_acceptance":false})
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_session(
    config: ProcessConfig,
    options: ClientOptions,
    root_uri: &str,
    document_uri: &str,
    source: &Path,
    data_directory: &Path,
    resolve_imports: bool,
    session: &str,
    previous_identity: Option<(u32, u64)>,
) -> SmokeResult<(u32, u64)> {
    let start = Instant::now();
    let client = LspClient::spawn(config, options)?;
    let pid = client.process_id();
    #[cfg(windows)]
    let observed = process_observer::ObservedProcess::open(pid)?;
    #[cfg(windows)]
    let identity = (pid, observed.creation_time);
    #[cfg(not(windows))]
    let identity = (pid, 0);
    let result = (|| -> SmokeResult<()> {
        let initialized = client.initialize(Some(root_uri), json!({"settings":{"java":{"import":{"gradle":{"enabled":false},"maven":{"enabled":false}}}}}))?;
        println!(
            "{}",
            json!({"kind":"initialize","session":session,"elapsed_ms":start.elapsed().as_millis(),"payload":initialized})
        );
        #[cfg(windows)]
        {
            observed.assert_live()?;
            if previous_identity == Some(identity) {
                return Err("restart reused the previous Java process identity".into());
            }
        }
        #[cfg(not(windows))]
        let _ = previous_identity;
        println!(
            "{}",
            json!({"kind":"process_identity","session":session,"pid":pid,
        "creation_time_100ns_since_1601": if cfg!(windows) { Some(identity.1) } else { None },
        "retained_windows_observation_handle":cfg!(windows)})
        );
        if resolve_imports
            && initialized
                .pointer("/capabilities/completionProvider/resolveProvider")
                .and_then(Value::as_bool)
                != Some(true)
        {
            return Err("--resolve-imports requires completion resolveProvider=true".into());
        }
        // Equinox must decode the file URL to this exact physical Unicode
        // data directory, not silently create a literal percent-escaped path.
        if !data_directory.join(".metadata").is_dir() {
            return Err(
                "JDT did not create metadata in the intended Unicode data directory".into(),
            );
        }
        println!(
            "{}",
            json!({"kind":"data_directory_witness","session":session,
            "data_directory":data_directory,"metadata_in_expected_directory":true})
        );
        sample_jvm_memory(client.process_id(), "after_initialize", start)?;
        client.did_open(document_uri, "java", 1, SOURCE)?;
        let initial_diagnostics = await_diagnostics(&client, document_uri, 1, true)?;
        let reference = SOURCE.rfind("greeting").ok_or("missing fixture marker")?;
        let hover = client.hover(document_uri, Position::end_of(&SOURCE[..reference + 2]))?;
        println!("{}", json!({"kind":"hover","payload":hover}));
        if !hover_contains(&hover, &["String", "greeting"]) {
            return Err("expected nonempty greeting hover".into());
        }
        let completion =
            client.completion(document_uri, Position::end_of(&SOURCE[..reference + 3]))?;
        println!("{}", json!({"kind":"completion","payload":completion}));
        let items = completion
            .as_array()
            .or_else(|| completion.get("items").and_then(Value::as_array))
            .ok_or("expected completion items")?;
        if !items.iter().any(|item| {
            item["label"]
                .as_str()
                .is_some_and(|label| label.starts_with("greeting"))
                && item.get("textEdit").is_some_and(Value::is_object)
        }) {
            return Err("expected greeting in real completion results".into());
        }
        if resolve_imports {
            let original = items
                .iter()
                .find(|item| {
                    item["label"]
                        .as_str()
                        .is_some_and(|label| label.starts_with("GregorianCalendar"))
                })
                .ok_or("expected GregorianCalendar candidate for lazy import resolution")?
                .clone();
            if !original.get("textEdit").is_some_and(Value::is_object)
                || original.get("data").is_none_or(Value::is_null)
            {
                return Err(
                    "expected original completion primary edit and opaque resolve data".into(),
                );
            }
            if original.get("additionalTextEdits").is_some_and(|edits| {
                !edits.is_null() && !edits.as_array().is_some_and(Vec::is_empty)
            }) {
                return Err(
                    "expected import edits to be deferred until completionItem/resolve".into(),
                );
            }
            let resolved = client.resolve_completion(original.clone())?;
            println!(
                "{}",
                json!({"kind":"completion_resolve","input":original,"payload":resolved,"server_command_executed":false})
            );
            let edits = resolved
                .get("additionalTextEdits")
                .and_then(Value::as_array)
                .ok_or("resolve did not return additionalTextEdits")?;
            if !edits.iter().any(|edit| {
                edit["newText"]
                    .as_str()
                    .is_some_and(|text| text.contains("import java.util.GregorianCalendar;"))
            }) {
                return Err(
                    "resolved item did not contain the expected GregorianCalendar import".into(),
                );
            }
            if original.get("textEdit") != resolved.get("textEdit")
                || original["label"] != resolved["label"]
            {
                return Err(
                    "server changed the completion's primary edit or label during resolve".into(),
                );
            }
            // Deliberately do not execute java.completion.onDidSelect or apply this
            // candidate to the fixture: the UI separately validates and applies edits.
        }
        let definition =
            client.definition(document_uri, Position::end_of(&SOURCE[..reference + 2]))?;
        println!("{}", json!({"kind":"definition","payload":definition}));
        if !exact_definition(&definition, document_uri, marker_range(SOURCE, "greeting")) {
            return Err("expected local greeting definition".into());
        }
        sample_jvm_memory(client.process_id(), "after_semantic_queries", start)?;
        assert_source_unchanged(source, "after_semantic_queries_and_resolve")?;
        let corrected = corrected_source();
        client.did_change(document_uri, 2, &corrected)?;
        let corrected_diagnostics = await_diagnostics(&client, document_uri, 2, false)?;
        let corrected_hover = client.hover(
            document_uri,
            marker_range(&corrected, "correctedOnly").start,
        )?;
        println!(
            "{}",
            json!({"kind":"corrected_hover","session":session,"payload":corrected_hover})
        );
        if !hover_contains(&corrected_hover, &["int", "correctedOnly"]) {
            return Err("expected corrected-draft hover for int correctedOnly".into());
        }
        assert_source_unchanged(source, "after_unsaved_correction")?;
        sample_jvm_memory(
            client.process_id(),
            "after_correction_before_shutdown",
            start,
        )?;
        client.did_close(document_uri)?;
        println!(
            "{}",
            json!({"kind":"session_semantics_pass","session":session,"elapsed_ms":start.elapsed().as_millis(),"initial_diagnostics":initial_diagnostics.diagnostics.len(),"corrected_diagnostics":corrected_diagnostics.diagnostics.len(),"lazy_import_resolve_checked":resolve_imports,"server_commands_executed":false,"checks":["initialize","didOpen","semantic_diagnostics","hover","completion","definition","didChange","diagnostic_error_cleared","correction_specific_warning","source_bytes_unchanged","didClose"]})
        );
        Ok(())
    })();
    // Always clean up before returning a semantic failure. Drop is the fallback
    // if initialize/close/shutdown failed. Assertions run before temp removal.
    let shutdown = client.shutdown();
    drop(client);
    #[cfg(windows)]
    let root_exit = observed.exit_code();
    #[cfg(not(windows))]
    let root_exit: SmokeResult<Option<u32>> = Ok(None);
    #[cfg(windows)]
    let gracefully_exited = Some(shutdown.is_ok() && matches!(&root_exit, Ok(0)));
    #[cfg(not(windows))]
    let gracefully_exited: Option<bool> = None;
    let disk = assert_source_unchanged(source, "after_shutdown_or_failure_drop");
    println!(
        "{}",
        json!({"kind":"session_cleanup","session":session,"pid":pid,
        "semantics_succeeded":result.is_ok(),"shutdown_api_succeeded":shutdown.is_ok(),
        "client_dropped":true,"windows_root_handle_signaled":cfg!(windows) && root_exit.is_ok(),
        "gracefully_exited":gracefully_exited,
        "root_exit_code":root_exit.as_ref().ok(),"source_unchanged":disk.is_ok(),
        "independent_job_zero_observation":false,"listener_observation":false})
    );
    result?;
    shutdown?;
    #[cfg(windows)]
    require_graceful_exit(root_exit?)?;
    #[cfg(not(windows))]
    root_exit?;
    disk?;
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = "file:///fixture%20%E9%9B%AA/src/Main.java";

    #[test]
    fn crash_report_arguments_are_scoped_ascii_and_disable_memory_dumps() {
        let temp = tempfile::tempdir().unwrap();
        if temp.path().to_str().is_some_and(str::is_ascii) {
            let args = crash_report_arguments(temp.path()).unwrap();
            assert_eq!(args.len(), 2);
            assert!(args[0].to_str().unwrap().ends_with("hs_err_pid%p.log"));
            assert_eq!(args[1], "-XX:-CreateCoredumpOnCrash");
        } else {
            assert!(crash_report_arguments(temp.path()).is_err());
        }
        for leaf in ["雪", "template%p"] {
            let path = temp.path().join(leaf);
            std::fs::create_dir(&path).unwrap();
            assert!(crash_report_arguments(&path).is_err());
        }
        assert!(crash_report_arguments(Path::new("relative")).is_err());
        assert!(crash_report_arguments(&temp.path().join("missing")).is_err());
    }

    #[test]
    fn relative_launcher_preserves_unicode_distribution_and_rejects_other_paths() {
        let temp = tempfile::Builder::new()
            .prefix("cedar launcher 雪 ")
            .tempdir()
            .unwrap();
        let plugins = temp.path().join("plugins");
        std::fs::create_dir(&plugins).unwrap();
        let jar = plugins.join("launcher.jar");
        std::fs::write(&jar, b"fixture").unwrap();
        assert_eq!(
            relative_launcher(temp.path(), &jar).unwrap(),
            Path::new("plugins").join("launcher.jar")
        );
        let unicode_jar = plugins.join("launcher雪.jar");
        std::fs::write(&unicode_jar, b"fixture").unwrap();
        assert!(relative_launcher(temp.path(), &unicode_jar).is_err());
        assert!(relative_launcher(&plugins, &temp.path().join("outside.jar")).is_err());
        assert!(relative_launcher(&jar, &jar).is_err());
        let uri = file_uri(temp.path()).unwrap();
        assert!(uri.is_ascii() && uri.contains("%E9%9B%AA"));
    }

    #[cfg(windows)]
    #[test]
    fn java_local_path_strips_only_verified_local_drive_prefixes() {
        use std::path::{Component, Prefix};
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().canonicalize().unwrap();
        let argument = java_local_path(&canonical).unwrap();
        assert!(matches!(argument.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))));
        assert_eq!(argument.canonicalize().unwrap(), canonical);
        for path in [
            r"\\server\share",
            r"\\?\UNC\server\share",
            r"\\.\NUL",
            r"C:relative",
        ] {
            assert!(ordinary_windows_local_path(Path::new(path)).is_err());
        }
    }

    #[test]
    fn graceful_exit_requires_zero_not_merely_a_signaled_root() {
        require_graceful_exit(0).unwrap();
        for code in [1, 259, 1067, u32::MAX] {
            assert!(require_graceful_exit(code).is_err());
        }
    }

    #[test]
    fn local_uri_matching_accepts_only_equivalent_fixture_paths() {
        assert!(same_local_uri("file:/fixture%20雪/src/Main.java", URI));
        assert!(same_local_uri(
            "file:///fixture%20%e9%9b%aa/src/Main.java",
            URI
        ));
        for wrong in [
            "file://host/fixture%20雪/src/Main.java",
            "file:////host/fixture%20雪/src/Main.java",
            "file:///other/Main.java",
            "file:///fixture%20雪/src/Main.java?x=1",
            "file:///fixture%20雪/src/Main.java#x",
            "file:///fixture%20雪/src/Main.java%00",
            "file:///fixture%ZZ雪/src/Main.java",
            "file:///fixture%20雪/src/%",
            "https:///fixture%20雪/src/Main.java",
        ] {
            assert!(!same_local_uri(wrong, URI), "{wrong}");
        }
        #[cfg(windows)]
        assert!(same_local_uri(
            "file:///c%3A/fixture/Main.java",
            "file:///C:/fixture/Main.java"
        ));
    }

    fn diagnostic(
        version: Option<i32>,
        severity: u32,
        message: &str,
        range: Range,
    ) -> PublishDiagnostics {
        serde_json::from_value(json!({"uri":URI,"version":version,
            "diagnostics":[{"range":range,"severity":severity,"message":message}]}))
        .unwrap()
    }

    #[test]
    fn initial_diagnostic_requires_exact_type_severity_range_uri_and_version() {
        let d = diagnostic(
            Some(1),
            1,
            "Type mismatch: cannot convert from String to int",
            marker_range(SOURCE, "\"oops\""),
        );
        assert!(diagnostics_match(&d, URI, 1, true));
        let mut wrong = d.clone();
        wrong.diagnostics[0].severity = Some(2);
        assert!(!diagnostics_match(&wrong, URI, 1, true));
        wrong = d.clone();
        wrong.diagnostics[0].range.start.character += 1;
        assert!(!diagnostics_match(&wrong, URI, 1, true));
        wrong = d.clone();
        wrong.diagnostics[0].message = "String and int mentioned in an unrelated error".into();
        assert!(!diagnostics_match(&wrong, URI, 1, true));
        assert!(!diagnostics_match(&d, "file:///other.java", 1, true));
        assert!(!diagnostics_match(&d, URI, 2, true));
        wrong = d.clone();
        wrong.version = Some(2);
        assert!(!diagnostics_match(&wrong, URI, 1, true));
    }

    #[test]
    fn clearing_requires_a_corrected_draft_witness_even_without_versions() {
        let mut d = diagnostic(
            None,
            2,
            "The value of the local variable correctedOnly is not used",
            marker_range(&corrected_source(), "correctedOnly"),
        );
        assert!(diagnostics_match(&d, URI, 2, false));
        d.version = Some(1);
        assert!(!diagnostics_match(&d, URI, 2, false));
        d.version = Some(2);
        assert!(diagnostics_match(&d, URI, 2, false));
        d.diagnostics[0].severity = Some(1);
        assert!(!diagnostics_match(&d, URI, 2, false));
        d.diagnostics[0].severity = Some(2);
        d.diagnostics[0].message = "The value of the local variable broken is not used".into();
        assert!(!diagnostics_match(&d, URI, 2, false));
        d.version = None;
        d.diagnostics.clear();
        assert!(!diagnostics_match(&d, URI, 2, false));
    }

    #[test]
    fn hover_requires_nonempty_recognized_contents_and_expected_symbol_type() {
        for contents in [
            json!("String greeting"),
            json!({"kind":"markdown","value":"String greeting"}),
            json!([{"language":"java","value":"String greeting"}]),
        ] {
            assert!(hover_contains(
                &json!({"contents":contents}),
                &["String", "greeting"]
            ));
        }
        for hover in [
            Value::Null,
            json!({}),
            json!({"contents":[]}),
            json!({"contents":"  "}),
            json!({"contents":{"other":"String greeting"}}),
            json!({"contents":"int greeting"}),
        ] {
            assert!(!hover_contains(&hover, &["String", "greeting"]));
        }
    }

    #[test]
    fn definition_requires_one_exact_local_declaration() {
        let range = marker_range(SOURCE, "greeting");
        let location = json!({"uri":URI,"range":range});
        assert!(exact_definition(&location, URI, range));
        assert!(exact_definition(&json!([location.clone()]), URI, range));
        assert!(exact_definition(
            &json!([{"targetUri":URI,"targetSelectionRange":range,"targetRange":range}]),
            URI,
            range
        ));
        assert!(!exact_definition(&location, "file:///external.java", range));
        assert!(!exact_definition(
            &location,
            URI,
            marker_range(SOURCE, "\"oops\"")
        ));
        for wrong in [
            Value::Null,
            json!([]),
            json!([location.clone(), location]),
            json!({"uri":URI}),
            json!({"targetUri":URI,"targetRange":range}),
        ] {
            assert!(!exact_definition(&wrong, URI, range));
        }
    }

    #[test]
    fn unicode_space_paths_are_encoded_and_source_comparison_detects_changes() {
        let temp = tempfile::Builder::new()
            .prefix("cedar unit 雪 ")
            .tempdir()
            .unwrap();
        let source = temp.path().join("Main.java");
        std::fs::write(&source, SOURCE).unwrap();
        let uri = file_uri(&source).unwrap();
        assert!(uri.starts_with("file:///"));
        assert!(uri.contains("%20"));
        assert!(uri.contains("%E9%9B%AA"));
        assert_source_unchanged(&source, "unit_original").unwrap();
        std::fs::write(&source, corrected_source()).unwrap();
        assert!(assert_source_unchanged(&source, "unit_changed").is_err());
        temp.close().unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_java_path_fallback_is_preserved() {
        assert_eq!(java_executable(None).unwrap(), Path::new("java"));
        assert_eq!(
            java_executable(Some(&"/jdk/bin/java".into())).unwrap(),
            Path::new("/jdk/bin/java")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_java_requires_existing_absolute_java_exe() {
        assert!(java_executable(None).is_err());
        assert!(java_executable(Some(&"java.exe".into())).is_err());
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("java.exe");
        assert!(java_executable(Some(&missing.as_os_str().to_owned())).is_err());
        // Path validation only: this unit test never tries to execute the file.
        std::fs::write(&missing, b"fixture").unwrap();
        let canonical = missing.canonicalize().unwrap();
        let executable = java_executable(Some(&missing.as_os_str().to_owned())).unwrap();
        assert_eq!(executable.canonicalize().unwrap(), canonical);
        assert!(matches!(
            executable.components().next(),
            Some(std::path::Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::Disk(_))
        ));
        assert_eq!(
            java_executable(Some(&canonical.as_os_str().to_owned())).unwrap(),
            executable
        );
        let unicode = temp.path().join("JDK 雪");
        std::fs::create_dir(&unicode).unwrap();
        let unicode = unicode.join("java.exe");
        std::fs::write(&unicode, b"fixture").unwrap();
        assert!(java_executable(Some(&unicode.as_os_str().to_owned())).is_err());
        let wrapper = temp.path().join("java.cmd");
        std::fs::write(&wrapper, b"fixture").unwrap();
        assert!(java_executable(Some(&wrapper.as_os_str().to_owned())).is_err());
    }

    #[test]
    fn proc_status_memory_fields_use_exact_names_and_kib_values() {
        let status = "Name:\tjava\nVmRSS:\t 262144 kB\nVmHWM:\t300000 kB\n";
        assert_eq!(status_kib(status, "VmRSS").unwrap(), 262144);
        assert_eq!(status_kib(status, "VmHWM").unwrap(), 300000);
    }

    #[test]
    fn missing_or_malformed_memory_measurements_are_errors() {
        for status in [
            "VmRSS: 3 MB",
            "VmRSS: unknown kB",
            "VmRSS: 12 kB extra",
            "OtherVmRSS: 12 kB",
            "",
        ] {
            assert!(status_kib(status, "VmRSS").is_err(), "{status}");
        }
    }
}
