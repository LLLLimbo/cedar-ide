//! Opt-in real-server test using an installed Eclipse JDT LS distribution.
//! Usage: cargo run -p cedar-language --example java_smoke -- /path/to/jdtls [/path/to/java]
//! Only a synthetic, temporary Eclipse Java project is created and inspected.
use cedar_language::{
    ClientOptions, LspClient, LspEvent, Position, ProcessConfig, PublishDiagnostics,
};
use serde_json::{json, Value};
use std::error::Error;
use std::path::Path;
use std::time::{Duration, Instant};

const SOURCE: &str = "public class Main {\n    public static void main(String[] args) {\n        String greeting = \"Hello Cedar\";\n        System.out.println(greeting);\n        int broken = \"oops\";\n    }\n}\n";

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
            Some(LspEvent::Diagnostics(d)) if d.uri == uri => {
                println!("{}", json!({"kind":"diagnostics","payload":d}));
                if d.version.is_some_and(|v| v < version) {
                    continue;
                }
                let has_type_error = d.diagnostics.iter().any(|d| {
                    d.severity == Some(1)
                        && d.message.contains("String")
                        && d.message.contains("int")
                });
                let has_any_error = d.diagnostics.iter().any(|d| d.severity == Some(1));
                if (expect_error && has_type_error) || (!expect_error && !has_any_error) {
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
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() || args.len() > 2 {
        eprintln!("Usage: java_smoke JDTLS_DIRECTORY [JAVA_EXECUTABLE]");
        std::process::exit(2);
    }
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
        .prefix("cedar-java-smoke-")
        .tempdir()?;
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::create_dir_all(project.join(".settings"))?;
    std::fs::write(project.join(".project"), "<?xml version=\"1.0\"?><projectDescription><name>cedar-language-smoke</name><projects/><buildSpec><buildCommand><name>org.eclipse.jdt.core.javabuilder</name><arguments/></buildCommand></buildSpec><natures><nature>org.eclipse.jdt.core.javanature</nature></natures></projectDescription>")?;
    std::fs::write(project.join(".classpath"), "<?xml version=\"1.0\"?><classpath><classpathentry kind=\"src\" path=\"src\"/><classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/><classpathentry kind=\"output\" path=\"bin\"/></classpath>")?;
    std::fs::write(project.join(".settings/org.eclipse.jdt.core.prefs"), "eclipse.preferences.version=1\norg.eclipse.jdt.core.compiler.codegen.targetPlatform=21\norg.eclipse.jdt.core.compiler.compliance=21\norg.eclipse.jdt.core.compiler.source=21\n")?;
    let source = project.join("src/Main.java");
    std::fs::write(&source, SOURCE)?;
    let root_uri = file_uri(&project)?;
    let document_uri = file_uri(&source)?;
    let mut config = ProcessConfig::new(args.get(1).cloned().unwrap_or_else(|| "java".into()));
    config.working_directory = Some(project);
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
        "-jar",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    config.args.push(jars[0].clone().into());
    config.args.push("-configuration".into());
    let platform = if cfg!(target_os = "macos") {
        "config_mac"
    } else if cfg!(target_os = "windows") {
        "config_win"
    } else {
        "config_linux"
    };
    config.args.push(jdtls.join(platform).into());
    config.args.push("-data".into());
    config.args.push(temp.path().join("jdt-data").into());
    let options = ClientOptions {
        request_timeout: Duration::from_secs(60),
        shutdown_timeout: Duration::from_secs(3),
        ..ClientOptions::default()
    };
    println!(
        "{}",
        json!({
            "kind":"run_metadata", "fixture":"fresh synthetic Eclipse Java project, one source file",
            "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
            "jvm_max_heap_mib":512, "memory_scope":"direct_language_server_jvm_only",
            "frontend_or_agent_included":false, "fresh_project_and_server_data":true
        })
    );
    let start = Instant::now();
    let client = LspClient::spawn(config, options)?;
    let initialized = client.initialize(Some(&root_uri), json!({"settings":{"java":{"import":{"gradle":{"enabled":false},"maven":{"enabled":false}}}}}))?;
    println!(
        "{}",
        json!({"kind":"initialize","elapsed_ms":start.elapsed().as_millis(),"payload":initialized})
    );
    sample_jvm_memory(client.process_id(), "after_initialize", start)?;
    client.did_open(&document_uri, "java", 1, SOURCE)?;
    let initial_diagnostics = await_diagnostics(&client, &document_uri, 1, true)?;
    let reference = SOURCE.rfind("greeting").ok_or("missing fixture marker")?;
    let hover = client.hover(&document_uri, Position::end_of(&SOURCE[..reference + 2]))?;
    println!("{}", json!({"kind":"hover","payload":hover}));
    if hover.is_null() || hover.get("contents").is_none() {
        return Err("expected nonempty greeting hover".into());
    }
    let completion =
        client.completion(&document_uri, Position::end_of(&SOURCE[..reference + 3]))?;
    println!("{}", json!({"kind":"completion","payload":completion}));
    let items = completion
        .as_array()
        .or_else(|| completion.get("items").and_then(Value::as_array))
        .ok_or("expected completion items")?;
    if !items.iter().any(|item| {
        item["label"]
            .as_str()
            .is_some_and(|label| label.starts_with("greeting"))
    }) {
        return Err("expected greeting in real completion results".into());
    }
    let definition =
        client.definition(&document_uri, Position::end_of(&SOURCE[..reference + 2]))?;
    println!("{}", json!({"kind":"definition","payload":definition}));
    if definition.is_null() || definition.as_array().is_some_and(Vec::is_empty) {
        return Err("expected local greeting definition".into());
    }
    sample_jvm_memory(client.process_id(), "after_semantic_queries", start)?;
    let corrected = SOURCE.replace("int broken = \"oops\";", "int broken = 42;");
    client.did_change(&document_uri, 2, &corrected)?;
    let corrected_diagnostics = await_diagnostics(&client, &document_uri, 2, false)?;
    sample_jvm_memory(
        client.process_id(),
        "after_correction_before_shutdown",
        start,
    )?;
    client.did_close(&document_uri)?;
    client.shutdown()?;
    println!(
        "{}",
        json!({"kind":"pass","elapsed_ms":start.elapsed().as_millis(),"initial_diagnostics":initial_diagnostics.diagnostics.len(),"corrected_diagnostics":corrected_diagnostics.diagnostics.len(),"checks":["initialize","didOpen","semantic_diagnostics","hover","completion","definition","didChange","diagnostic_error_cleared","didClose","shutdown"]})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::status_kib;

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
