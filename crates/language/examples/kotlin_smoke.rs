//! Opt-in real-server test for the community fwcd/kotlin-language-server 1.3.13.
//! Usage: cargo run -p cedar-language --example kotlin_smoke -- /path/to/server [/path/to/java]
//! This is NOT a validation of JetBrains' official Kotlin/kotlin-lsp server.
//! Only a fresh synthetic project is opened; no build files or scripts are created.
use cedar_language::{
    ClientOptions, LspClient, LspEvent, Position, ProcessConfig, PublishDiagnostics,
};
use serde_json::{json, Value};
use std::error::Error;
use std::path::Path;
use std::time::{Duration, Instant};

const SOURCE: &str = "fun main() {\n    val greeting: String = \"Hello Cedar\"\n    println(greeting)\n    val broken: Int = \"oops\"\n    println(broken)\n}\n";

fn file_uri(path: &Path) -> Result<String, Box<dyn Error>> {
    let path = path.canonicalize()?;
    let text = path
        .to_str()
        .ok_or("fixture path must be UTF-8")?
        .replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let mut uri = if text.starts_with('/') {
        "file://"
    } else {
        "file:///"
    }
    .to_owned();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(uri)
}

fn sample_memory(pid: u32, phase: &str, start: Instant) -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "linux")]
    {
        let source = format!("/proc/{pid}/status");
        let status = std::fs::read_to_string(&source)?;
        let kib = |field: &str| -> Result<u64, Box<dyn Error>> {
            let line = status
                .lines()
                .find(|line| line.starts_with(field))
                .ok_or("missing process memory field")?;
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() != 3 || parts[2] != "kB" {
                return Err("unexpected process memory units".into());
            }
            Ok(parts[1].parse()?)
        };
        let rss = kib("VmRSS:")?;
        let hwm = kib("VmHWM:")?;
        println!(
            "{}",
            json!({"kind":"jvm_memory","phase":phase,"elapsed_ms":start.elapsed().as_millis(),"pid":pid,"source":source,"scope":"direct_language_server_jvm_only","vm_rss_kib":rss,"vm_hwm_kib":hwm,"vm_rss_mib":rss as f64/1024.0,"vm_hwm_mib":hwm as f64/1024.0})
        );
    }
    #[cfg(not(target_os = "linux"))]
    println!(
        "{}",
        json!({"kind":"jvm_memory_unavailable","phase":phase,"pid":pid,"elapsed_ms":start.elapsed().as_millis(),"reason":"Linux /proc measurement only"})
    );
    Ok(())
}

fn is_type_error(d: &cedar_language::Diagnostic) -> bool {
    d.severity == Some(1)
        && (d.code.as_ref().and_then(Value::as_str) == Some("TYPE_MISMATCH")
            || (d.message.contains("String") && d.message.contains("Int")))
}

fn await_diagnostics(
    client: &LspClient,
    uri: &str,
    version: i32,
    expect_error: bool,
) -> Result<PublishDiagnostics, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline {
        match client.next_event(Duration::from_millis(500))? {
            Some(LspEvent::Diagnostics(d)) if d.uri == uri => {
                println!(
                    "{}",
                    json!({"kind":"diagnostics","expected_version":version,"payload":d})
                );
                if d.version.is_some_and(|v| v < version) {
                    continue;
                }
                if (expect_error && d.diagnostics.iter().any(is_type_error))
                    || (!expect_error && !d.diagnostics.iter().any(|d| d.severity == Some(1)))
                {
                    return Ok(d);
                }
            }
            Some(LspEvent::Closed(error)) => return Err(error.into()),
            Some(LspEvent::Lagged { dropped }) => {
                return Err(format!("lost {dropped} events").into())
            }
            Some(LspEvent::UnsupportedServerRequest { method, .. }) => println!(
                "{}",
                json!({"kind":"unsupported_callback","method":method,"executed":false})
            ),
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

fn validate(client: &LspClient, document_uri: &str, start: Instant) -> Result<(), Box<dyn Error>> {
    client.did_open(document_uri, "kotlin", 1, SOURCE)?;
    let initial = await_diagnostics(client, document_uri, 1, true)?;
    let reference = SOURCE
        .find("println(greeting)")
        .ok_or("missing fixture reference")?
        + "println(".len();
    let position = Position::end_of(&SOURCE[..reference + 3]);
    let hover = client.hover(document_uri, position)?;
    println!("{}", json!({"kind":"hover","payload":hover}));
    if !hover
        .get("contents")
        .is_some_and(|value| value.to_string().contains("String"))
    {
        return Err("hover did not report the greeting String type".into());
    }
    let completion = client.completion(
        document_uri,
        Position::end_of(&SOURCE[..reference + "greeting".len()]),
    )?;
    println!("{}", json!({"kind":"completion","payload":completion}));
    let items = completion
        .as_array()
        .or_else(|| completion.get("items").and_then(Value::as_array))
        .ok_or("missing completion items")?;
    if !items.iter().any(|item| {
        item["label"]
            .as_str()
            .is_some_and(|label| label.starts_with("greeting"))
    }) {
        return Err("completion did not include greeting".into());
    }
    let definition = client.definition(document_uri, position)?;
    println!("{}", json!({"kind":"definition","payload":definition}));
    let locations = definition
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![definition.clone()]);
    if !locations.iter().any(|location| {
        (location["uri"] == document_uri
            && location
                .pointer("/range/start/line")
                .and_then(Value::as_u64)
                == Some(1))
            || (location["targetUri"] == document_uri
                && location
                    .pointer("/targetSelectionRange/start/line")
                    .and_then(Value::as_u64)
                    == Some(1))
    }) {
        return Err("definition did not resolve to the local greeting declaration".into());
    }
    sample_memory(client.process_id(), "after_semantic_queries", start)?;
    // Drain queued pre-edit notifications so an old empty diagnostic batch cannot
    // be mistaken for the correction. fwcd 1.3.13 sends unversioned diagnostics.
    while let Some(event) = client.next_event(Duration::ZERO)? {
        match event {
            LspEvent::Closed(error) => return Err(error.into()),
            LspEvent::Lagged { dropped } => return Err(format!("lost {dropped} events").into()),
            _ => println!(
                "{}",
                json!({"kind":"pre_edit_event","payload":format!("{event:?}")})
            ),
        }
    }
    let corrected = SOURCE.replace("val broken: Int = \"oops\"", "val broken: Int = 42");
    client.did_change(document_uri, 2, &corrected)?;
    let corrected_diagnostics = await_diagnostics(client, document_uri, 2, false)?;
    sample_memory(
        client.process_id(),
        "after_correction_before_shutdown",
        start,
    )?;
    client.did_close(document_uri)?;
    println!(
        "{}",
        json!({"kind":"semantic_checks_passed","initial_diagnostics":initial.diagnostics.len(),"corrected_diagnostics":corrected_diagnostics.diagnostics.len(),"diagnostics_have_versions":initial.version.is_some() && corrected_diagnostics.version.is_some()})
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() || args.len() > 2 {
        return Err("Usage: kotlin_smoke FWCD_SERVER_DIRECTORY [JAVA_EXECUTABLE]".into());
    }
    let server = Path::new(&args[0]).canonicalize()?;
    if !server.join("lib/server-1.3.13.jar").is_file() {
        return Err(
            "expected unpacked fwcd server 1.3.13 directory containing lib/server-1.3.13.jar"
                .into(),
        );
    }
    let java = args.get(1).cloned().unwrap_or_else(|| "java".into());
    let java_version = std::process::Command::new(&java).arg("-version").output()?;
    if !java_version.status.success() {
        return Err("could not query Java version".into());
    }
    let temp = tempfile::Builder::new()
        .prefix("cedar-kotlin-smoke-")
        .tempdir()?;
    let project = temp.path().join("project");
    let home = temp.path().join("home");
    let config_home = home.join(".config");
    std::fs::create_dir_all(&project)?;
    std::fs::create_dir_all(&config_home)?;
    // Supply exactly the distribution's bundled stdlib in a private Maven-style
    // cache. The server's documented backup resolver reads it without running
    // Maven, Gradle, project scripts or downloading any dependencies.
    let maven_repository = home.join(".m2/repository");
    let stdlib_dir = maven_repository.join("org/jetbrains/kotlin/kotlin-stdlib/2.1.0");
    std::fs::create_dir_all(&stdlib_dir)?;
    std::fs::copy(
        server.join("lib/kotlin-stdlib-2.1.0.jar"),
        stdlib_dir.join("kotlin-stdlib-2.1.0.jar"),
    )?;
    let source = project.join("Main.kt");
    std::fs::write(&source, SOURCE)?;
    let root_uri = file_uri(&project)?;
    let document_uri = file_uri(&source)?;
    // env execs Java with the same PID, so /proc samples measure the JVM itself.
    // Isolate global classpath scripts/config in a newly created empty directory.
    #[cfg(unix)]
    let mut config = {
        let mut config = ProcessConfig::new("/usr/bin/env");
        config.args = vec![
            "-u".into(),
            "JAVA_TOOL_OPTIONS".into(),
            "-u".into(),
            "_JAVA_OPTIONS".into(),
            "-u".into(),
            "JDK_JAVA_OPTIONS".into(),
            format!("HOME={}", home.display()).into(),
            format!("MAVEN_REPOSITORY={}", maven_repository.display()).into(),
            format!("GRADLE_USER_HOME={}", home.join(".gradle").display()).into(),
            format!("XDG_CONFIG_HOME={}", config_home.display()).into(),
            java,
        ];
        config
    };
    #[cfg(not(unix))]
    let mut config = ProcessConfig::new(java);
    config.working_directory = Some(project);
    config.args.extend([
        "-Xms64m".into(),
        "-Xmx512m".into(),
        "-DkotlinLanguageServer.version=1.3.13".into(),
        format!("-Duser.home={}", home.display()).into(),
        "-cp".into(),
        server.join("lib/*").into(),
        "org.javacs.kt.MainKt".into(),
    ]);
    let options = ClientOptions {
        request_timeout: Duration::from_secs(60),
        shutdown_timeout: Duration::from_secs(5),
        ..ClientOptions::default()
    };
    println!(
        "{}",
        json!({"kind":"run_metadata","server":"fwcd/kotlin-language-server","server_version":"1.3.13","official_jetbrains_server":false,"fixture":"fresh synthetic Kotlin folder, one source file, no build files or scripts","java_version":String::from_utf8_lossy(&java_version.stderr),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"jvm_max_heap_mib":512,"memory_scope":"direct_language_server_jvm_only","frontend_or_agent_included":false,"fresh_project_and_server_data":true,"fixture_source":SOURCE,"fixture_stdlib":"bundled kotlin-stdlib-2.1.0.jar copied to isolated Maven-style cache; no build invocation"})
    );
    let start = Instant::now();
    let client = LspClient::spawn(config, options)?;
    let initialized = client.initialize_with_timeout(
        Some(&root_uri),
        json!({"storagePath":temp.path().join("server-data")}),
        Duration::from_secs(90),
    )?;
    println!(
        "{}",
        json!({"kind":"initialize","elapsed_ms":start.elapsed().as_millis(),"payload":initialized})
    );
    sample_memory(client.process_id(), "after_initialize", start)?;
    let result = validate(&client, &document_uri, start);
    // Always attempt orderly shutdown, even when a semantic assertion failed.
    let shutdown_start = Instant::now();
    let shutdown = client.shutdown();
    let mut terminal_reason = None;
    loop {
        match client.next_event(Duration::ZERO) {
            Ok(Some(LspEvent::Closed(error))) | Err(error) => {
                terminal_reason = Some(error.to_string());
                break;
            }
            Ok(None) => break,
            Ok(Some(_)) => {}
        }
    }
    #[cfg(target_os = "linux")]
    let child_removed = !Path::new(&format!("/proc/{}", client.process_id())).exists();
    #[cfg(not(target_os = "linux"))]
    let child_removed = shutdown.is_ok();
    println!(
        "{}",
        json!({"kind":"shutdown_cleanup","elapsed_ms":shutdown_start.elapsed().as_millis(),"terminal_reason":terminal_reason,"direct_child_removed":child_removed,"grace_period_ms":5000})
    );
    result?;
    shutdown?;
    if !child_removed {
        return Err("language server process still present after shutdown".into());
    }
    let fixture_path = temp.path().to_path_buf();
    temp.close()?;
    println!(
        "{}",
        json!({"kind":"pass","elapsed_ms":start.elapsed().as_millis(),"temporary_fixture_removed":!fixture_path.exists(),"server_commands_executed":false,"workspace_apply_edit_executed":false,"checks":["initialize","didOpen","semantic_type_diagnostic","typed_hover","completion","local_definition","didChange","diagnostic_errors_cleared","didClose","shutdown_request_and_process_cleanup","temporary_fixture_cleanup"]})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_uri_escapes_spaces_and_unicode() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("a space λ.kt");
        std::fs::write(&file, "").unwrap();
        let uri = file_uri(&file).unwrap();
        assert!(uri.starts_with("file:///"));
        assert!(uri.ends_with("a%20space%20%CE%BB.kt"));
    }

    #[test]
    fn diagnostic_assertion_requires_a_real_error() {
        let d: cedar_language::Diagnostic = serde_json::from_value(json!({
            "range":{"start":{"line":3,"character":22},"end":{"line":3,"character":28}},
            "severity":1,"code":"TYPE_MISMATCH",
            "message":"Type mismatch: inferred type is String but Int was expected"
        }))
        .unwrap();
        assert!(is_type_error(&d));
        let mut warning = d.clone();
        warning.severity = Some(2);
        assert!(!is_type_error(&warning));
        let mut other = d;
        other.code = Some(json!("UNRESOLVED_REFERENCE"));
        other.message = "Unresolved reference: println".into();
        assert!(!is_type_error(&other));
    }
}
