//! A bounded view of the one explicitly selected Maven leaf project.
//! Server paths are metadata, never authority to read files or run commands.
use super::{error, java_diagnostics_refresh_supported, Workspace};
use crate::java_maven::{current_pom_matches, MavenSession};
use cedar_protocol::{Payload, RemoteError};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const COMMAND: &str = "java.project.getSettings";
const NATURES: &str = "org.eclipse.jdt.ls.core.natureIds";
const SOURCES: &str = "org.eclipse.jdt.ls.core.sourcePaths";
const CLASSPATH: &str = "org.eclipse.jdt.ls.core.classpathEntries";
const SOURCE: &str = "org.eclipse.jdt.core.compiler.source";
const COMPLIANCE: &str = "org.eclipse.jdt.core.compiler.compliance";
const TARGET: &str = "org.eclipse.jdt.core.compiler.codegen.targetPlatform";
const RELEASE: &str = "org.eclipse.jdt.core.compiler.release";
const MAX_SOURCES: usize = 64;
const MAX_CLASSPATH: usize = 256;
const MAX_PATH: usize = 4096;
const MAX_COMPILER: usize = 32;
const MAX_JSON: usize = 128 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const UNAVAILABLE: &str =
    "Maven project settings are unavailable. Try again after import completes.";

pub(super) fn supported(typed_maven: bool, initialize: &Value) -> bool {
    java_diagnostics_refresh_supported(typed_maven, initialize)
        && initialize["capabilities"]["executeCommandProvider"]["commands"]
            .as_array()
            .is_some_and(|commands| {
                commands.iter().all(Value::is_string)
                    && commands
                        .iter()
                        .any(|command| command.as_str() == Some(COMMAND))
            })
}

impl Workspace {
    pub(super) fn maven_model(&self) -> Result<Payload, RemoteError> {
        let session = self
            .language
            .as_ref()
            .ok_or_else(|| error("language_not_running", "Start a language server first"))?;
        let maven = session
            .java_maven
            .as_ref()
            .filter(|_| session.production_java)
            .ok_or_else(|| {
                error(
                    "language_maven_session_required",
                    "Maven project settings require a typed Maven Java session",
                )
            })?;
        let value = query_model(maven, session.java_maven_model, |params, timeout| {
            session
                .client
                .request_with_timeout("workspace/executeCommand", params, timeout)
                .map_err(|_| ())
        })?;
        Ok(Payload::Language { value })
    }
}

fn query_model(
    maven: &MavenSession,
    command_supported: bool,
    request: impl FnOnce(Value, Duration) -> Result<Value, ()>,
) -> Result<Value, RemoteError> {
    current_pom_matches(maven)?;
    if !command_supported {
        return Err(error(
            "language_maven_unsupported",
            "The typed Maven session does not advertise supported project settings",
        ));
    }
    // No caller-controlled URI, command, key, retry, save, or reload is accepted.
    let result = request(
        json!({"command":COMMAND,"arguments":[maven.pom_uri,
            [NATURES,SOURCES,CLASSPATH,SOURCE,COMPLIANCE,TARGET,RELEASE]]}),
        REQUEST_TIMEOUT,
    );
    // A failed or timed-out request must not hide a POM change either.
    current_pom_matches(maven)?;
    match result {
        Ok(value) => normalize_model(maven, value),
        Err(()) => Ok(unavailable(maven)),
    }
}

fn invalid() -> RemoteError {
    error(
        "language_maven_invalid_model",
        "Maven project settings contain unsupported or oversized metadata",
    )
}

fn summary(
    maven: &MavenSession,
    status: &str,
    compiler: Value,
    sources: Vec<String>,
    classpath: Vec<Value>,
    unresolved: usize,
) -> Value {
    json!({"profile":"maven_leaf","status":status,"pom_path":"pom.xml",
        "pom_sha256":maven.pom_sha256,"restart_required":false,"maven_nature":status != "unavailable","compiler":compiler,
        "source_paths":sources,"classpath":classpath,"unresolved_count":unresolved})
}

fn unavailable(maven: &MavenSession) -> Value {
    let mut value = summary(
        maven,
        "unavailable",
        json!({"source":null,"compliance":null,"target":null,"release_enabled":null}),
        Vec::new(),
        Vec::new(),
        0,
    );
    value["message"] = json!(UNAVAILABLE);
    value
}

fn incomplete_model(maven: &MavenSession) -> Value {
    let mut value = unavailable(maven);
    value["maven_nature"] = json!(true);
    value
}

// Count serialized bytes without allocating an unbounded second server payload.
fn bounded_json(value: &Value) -> Result<(), RemoteError> {
    struct Budget(usize);
    impl Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| io::Error::other("JSON limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(MAX_JSON), value).map_err(|_| invalid())
}

fn compiler_text<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, RemoteError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value))
            if !value.is_empty()
                && value.len() <= MAX_COMPILER
                && value.bytes().all(|byte| (b' '..=b'~').contains(&byte)) =>
        {
            Ok(Some(value))
        }
        _ => Err(invalid()),
    }
}

fn normalize_model(maven: &MavenSession, value: Value) -> Result<Value, RemoteError> {
    // Unknown keys and attributes are ignored only inside this whole-response cap.
    bounded_json(&value)?;
    let Some(object) = value.as_object() else {
        return if value.is_null() {
            Ok(unavailable(maven))
        } else {
            Err(invalid())
        };
    };
    let Some(natures) = object.get(NATURES).filter(|value| !value.is_null()) else {
        return Ok(unavailable(maven));
    };
    let natures = natures.as_array().ok_or_else(invalid)?;
    if natures.len() > MAX_SOURCES
        || natures.iter().any(|nature| {
            nature
                .as_str()
                .is_none_or(|nature| nature.len() > 256 || nature.chars().any(char::is_control))
        })
    {
        return Err(invalid());
    }
    if !natures
        .iter()
        .any(|nature| nature.as_str() == Some("org.eclipse.m2e.core.maven2Nature"))
    {
        return Ok(unavailable(maven));
    }
    let source = compiler_text(object, SOURCE)?;
    let compliance = compiler_text(object, COMPLIANCE)?;
    let target = compiler_text(object, TARGET)?;
    let release = match compiler_text(object, RELEASE)? {
        Some("enabled") => Some(true),
        Some("disabled") => Some(false),
        None => None,
        Some(_) => return Err(invalid()),
    };
    let (Some(source), Some(compliance), Some(target)) = (source, compliance, target) else {
        return Ok(incomplete_model(maven));
    };
    let (Some(sources), Some(classpath)) = (
        object.get(SOURCES).filter(|value| !value.is_null()),
        object.get(CLASSPATH).filter(|value| !value.is_null()),
    ) else {
        return Ok(incomplete_model(maven));
    };
    let sources = sources.as_array().ok_or_else(invalid)?;
    let classpath = classpath.as_array().ok_or_else(invalid)?;
    if sources.len() > MAX_SOURCES
        || classpath.len() > MAX_CLASSPATH
        || maven.declared_dependencies.len() > MAX_CLASSPATH
    {
        return Err(invalid());
    }
    if sources.is_empty() {
        return Ok(incomplete_model(maven));
    }
    let root = checked_path(path_text(&maven.root)?, true)?;
    let repository = checked_path(path_text(&maven.local_repository)?, true)?;
    if !root.exists || !repository.exists {
        return Err(invalid());
    }
    let mut source_paths = Vec::new();
    let mut source_keys = HashSet::new();
    for source in sources {
        let source = confined_path(source.as_str().ok_or_else(invalid)?, true, &root, None)?;
        let relative = relative_to(&source, &root).ok_or_else(invalid)?;
        if source_keys.insert(source.key.clone()) {
            source_paths.push(relative);
        }
    }
    if let Some(main) = maven.source_paths.first() {
        let main = confined_path(path_text(&maven.root.join(main))?, true, &root, None)?;
        if main.exists && !source_keys.contains(&main.key) {
            return Ok(incomplete_model(maven));
        }
    }
    let mut entries = Vec::new();
    let mut entry_keys = HashMap::new();
    for entry in classpath {
        let entry = entry.as_object().ok_or_else(invalid)?;
        let kind = match entry.get("kind").and_then(Value::as_u64) {
            Some(1) => "library",
            Some(3) => "source",
            // Project, variable, and container entries are not in the audited
            // getSettings response. Never guess at their resolution or roots.
            _ => return Err(invalid()),
        };
        let path = confined_path(
            entry
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?,
            kind == "source",
            &root,
            (kind == "library").then_some(&repository),
        )?;
        let display = if kind == "source" {
            relative_to(&path, &root).ok_or_else(invalid)?
        } else {
            path.text.clone()
        };
        insert_entry(&mut entries, &mut entry_keys, path, kind, display, "model")?;
    }
    // Some JDT states omit unresolved artifacts. A declared coordinate still
    // claims its captured cache path, and its absence must remain visible.
    let mut incomplete_dependencies = false;
    for declared in &maven.declared_dependencies {
        let path = confined_path(path_text(declared)?, false, &root, Some(&repository))?;
        if path.exists && !entry_keys.contains_key(&path.key) {
            incomplete_dependencies = true;
        }
        let display = path.text.clone();
        insert_entry(
            &mut entries,
            &mut entry_keys,
            path,
            "library",
            display,
            "declared",
        )?;
    }
    // Merely finding a cached JAR does not establish that JDT imported it.
    if incomplete_dependencies {
        return Ok(incomplete_model(maven));
    }
    let unresolved = entries
        .iter()
        .filter(|entry| entry["kind"] == "library" && entry["resolved"] == false)
        .count();
    let mut result = summary(
        maven,
        if unresolved == 0 {
            "imported"
        } else {
            "unresolved"
        },
        json!({"source":source,"compliance":compliance,"target":target,"release_enabled":release}),
        source_paths,
        entries,
        unresolved,
    );
    if unresolved != 0 {
        result["message"] = json!("One or more dependency artifacts are missing from the selected project or local repository.");
    }
    bounded_json(&result)?;
    Ok(result)
}

fn insert_entry(
    entries: &mut Vec<Value>,
    keys: &mut HashMap<String, (&'static str, usize)>,
    path: CheckedPath,
    kind: &'static str,
    display: String,
    origin: &str,
) -> Result<(), RemoteError> {
    if let Some((previous, index)) = keys.get(&path.key) {
        if *previous != kind {
            return Err(invalid());
        }
        // A later observation of absence wins over an earlier present entry.
        if !path.exists {
            entries[*index]["resolved"] = json!(false);
        }
        return Ok(());
    }
    if entries.len() >= MAX_CLASSPATH {
        return Err(invalid());
    }
    keys.insert(path.key, (kind, entries.len()));
    entries.push(json!({"kind":kind,"path":display,"resolved":path.exists,"origin":origin}));
    Ok(())
}

struct CheckedPath {
    text: String,
    key: String,
    exists: bool,
}

fn path_text(path: &Path) -> Result<&str, RemoteError> {
    path.to_str().ok_or_else(invalid)
}

fn path_key(text: &str) -> String {
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text.to_owned()
    }
}

fn relative_to(path: &CheckedPath, root: &CheckedPath) -> Option<String> {
    if path.key == root.key {
        return Some(".".into());
    }
    let prefix = format!("{}/", root.key.trim_end_matches('/'));
    path.key
        .starts_with(&prefix)
        .then(|| path.text[prefix.len()..].to_owned())
}

fn confined_path(
    text: &str,
    directory: bool,
    root: &CheckedPath,
    repository: Option<&CheckedPath>,
) -> Result<CheckedPath, RemoteError> {
    let text = normalized_spelling(text, cfg!(windows))?;
    let lexical = CheckedPath {
        key: path_key(&text),
        text,
        exists: false,
    };
    if relative_to(&lexical, root).is_none()
        && !repository.is_some_and(|repository| relative_to(&lexical, repository).is_some())
    {
        return Err(invalid());
    }
    checked_path(&lexical.text, directory)
}

fn normalized_spelling(text: &str, windows: bool) -> Result<String, RemoteError> {
    if text.is_empty() || text.len() > MAX_PATH || text.chars().any(char::is_control) {
        return Err(invalid());
    }
    let text = if windows {
        text.replace('\\', "/")
    } else {
        if text.contains('\\') {
            return Err(invalid());
        }
        text.to_owned()
    };
    let rest = if windows {
        let bytes = text.as_bytes();
        if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1..3] != *b":/" {
            return Err(invalid());
        }
        &text[3..]
    } else {
        text.strip_prefix('/').ok_or_else(invalid)?
    };
    if !rest.is_empty()
        && rest.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || (windows && invalid_windows_component(part))
        })
    {
        return Err(invalid());
    }
    Ok(text)
}

fn invalid_windows_component(part: &str) -> bool {
    if part.ends_with(['.', ' ']) || part.contains([':', '<', '>', '"', '|', '?', '*']) {
        return true;
    }
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

fn checked_path(text: &str, directory: bool) -> Result<CheckedPath, RemoteError> {
    let text = normalized_spelling(text, cfg!(windows))?;
    let path = PathBuf::from(&text);
    let mut exists = true;
    let mut deepest = None;
    // Walk from the volume root, including ancestors of both selected roots.
    // Missing tails are allowed; existing symlinks, reparse points, short-name
    // aliases, non-directory parents, and unexpected file kinds are rejected.
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                exists = false;
                break;
            }
            Err(_) => return Err(invalid()),
        };
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = false;
        if metadata.file_type().is_symlink()
            || reparse
            || if ancestor != path.as_path() || directory {
                !metadata.is_dir()
            } else {
                !metadata.is_file()
            }
        {
            return Err(invalid());
        }
        deepest = Some(ancestor);
    }
    if let Some(ancestor) = deepest {
        let canonical = fs::canonicalize(ancestor).map_err(|_| invalid())?;
        let canonical = path_text(&canonical)?;
        #[cfg(windows)]
        let canonical = canonical.strip_prefix(r"\\?\").unwrap_or(canonical);
        let canonical = normalized_spelling(canonical, cfg!(windows))?;
        let expected = normalized_spelling(path_text(ancestor)?, cfg!(windows))?;
        if path_key(&canonical) != path_key(&expected) {
            return Err(invalid());
        }
    }
    Ok(CheckedPath {
        key: path_key(&text),
        text,
        exists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn fixture() -> (tempfile::TempDir, MavenSession) {
        let temp = tempfile::tempdir().unwrap();
        let base = crate::java_launch::ordinary_local_path(temp.path()).unwrap();
        let root = base.join("project");
        let repository = base.join("repository");
        fs::create_dir_all(root.join("src/main/java")).unwrap();
        fs::create_dir_all(&repository).unwrap();
        let pom = b"<project/>";
        fs::write(root.join("pom.xml"), pom).unwrap();
        let session = MavenSession {
            pom_uri: url::Url::from_file_path(root.join("pom.xml"))
                .unwrap()
                .into(),
            root,
            local_repository: repository,
            pom_sha256: format!("{:x}", Sha256::digest(pom)),
            declared_dependencies: Vec::new(),
            source_paths: vec!["src/main/java".into()],
        };
        (temp, session)
    }

    fn model(maven: &MavenSession) -> Value {
        let mut value = json!({});
        value[NATURES] = json!([
            "org.eclipse.jdt.core.javanature",
            "org.eclipse.m2e.core.maven2Nature"
        ]);
        value[SOURCES] = json!([maven.root.join("src/main/java")]);
        value[CLASSPATH] = json!([{"kind":3,"path":maven.root.join("src/main/java")}]);
        value[SOURCE] = json!("17");
        value[COMPLIANCE] = json!("17");
        value[TARGET] = json!("17");
        value[RELEASE] = json!("enabled");
        value
    }

    #[test]
    fn support_requires_exact_identity_typed_session_and_advertised_command() {
        let initialize = json!({"serverInfo":{"name":"JDT Language Server (Standard)","version":"1.61.0-SNAPSHOT"},"capabilities":{"executeCommandProvider":{"commands":[COMMAND]}}});
        assert!(supported(true, &initialize));
        assert!(!supported(false, &initialize));
        for info in [
            Value::Null,
            json!({"name":"JDT Language Server (Syntax)","version":"1.61.0-SNAPSHOT"}),
            json!({"name":"JDT Language Server (Standard)","version":"1.61.0"}),
        ] {
            let mut value = initialize.clone();
            value["serverInfo"] = info;
            assert!(!supported(true, &value));
        }
        for commands in [
            Value::Null,
            json!([]),
            json!([COMMAND, true]),
            json!(["java.project.getSettings.other"]),
            json!(["java.project.updateSettings"]),
        ] {
            let mut value = initialize.clone();
            value["capabilities"]["executeCommandProvider"]["commands"] = commands;
            assert!(!supported(true, &value));
        }
    }

    #[test]
    fn missing_library_is_unresolved_even_when_jdt_claims_library_kind() {
        let (_temp, mut maven) = fixture();
        let missing = maven
            .local_repository
            .join("group/artifact/1/artifact-1.jar");
        maven.declared_dependencies.push(missing.clone());
        let mut raw = model(&maven);
        raw[CLASSPATH]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":1,"path":missing,"resolved":true}));
        let value = normalize_model(&maven, raw).unwrap();
        assert_eq!(value["status"], "unresolved");
        assert_eq!(value["unresolved_count"], 1);
        assert_eq!(value["classpath"].as_array().unwrap().len(), 2);
        assert_eq!(value["classpath"][1]["resolved"], false);
        assert_eq!(value["classpath"][1]["origin"], "model");
    }

    #[test]
    fn omitted_declared_libraries_and_missing_source_folders_have_distinct_status() {
        let (_temp, mut maven) = fixture();
        let missing = maven.local_repository.join("missing.jar");
        maven.declared_dependencies = vec![missing.clone(), missing.clone()];
        let mut raw = model(&maven);
        raw[CLASSPATH]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":3,"path":maven.root.join("src/test/java")}));
        let value = normalize_model(&maven, raw.clone()).unwrap();
        assert_eq!(value["status"], "unresolved");
        assert_eq!(value["unresolved_count"], 1);
        assert_eq!(value["classpath"][1]["resolved"], false);
        assert_eq!(value["classpath"][2]["origin"], "declared");
        fs::write(missing, b"fixture").unwrap();
        let value = normalize_model(&maven, raw.clone()).unwrap();
        assert_eq!(value["status"], "unavailable");
        raw[CLASSPATH]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":1,"path":maven.declared_dependencies[0]}));
        let value = normalize_model(&maven, raw).unwrap();
        assert_eq!(value["status"], "imported");
        assert_eq!(value["unresolved_count"], 0);
    }

    #[test]
    fn sources_are_relative_and_root_is_dot() {
        let (_temp, maven) = fixture();
        let mut raw = model(&maven);
        raw[SOURCES] = json!([maven.root, maven.root.join("src/main/java"), maven.root]);
        let value = normalize_model(&maven, raw).unwrap();
        assert_eq!(value["source_paths"], json!([".", "src/main/java"]));
        assert_eq!(value["maven_nature"], true);
        assert_eq!(
            value["compiler"],
            json!({"source":"17","compliance":"17","target":"17","release_enabled":true})
        );
    }

    #[test]
    fn incomplete_and_unknown_models_never_appear_imported() {
        let (_temp, maven) = fixture();
        for value in [Value::Null, json!({})] {
            let value = normalize_model(&maven, value).unwrap();
            assert_eq!(value["status"], "unavailable");
            assert_eq!(value["maven_nature"], false);
        }
        for key in [NATURES, SOURCES, CLASSPATH, SOURCE, COMPLIANCE, TARGET] {
            let mut value = model(&maven);
            value.as_object_mut().unwrap().remove(key);
            assert_eq!(
                normalize_model(&maven, value).unwrap()["status"],
                "unavailable"
            );
        }
        let mut value = model(&maven);
        value[NATURES] = json!(["org.eclipse.jdt.core.javanature"]);
        assert_eq!(
            normalize_model(&maven, value).unwrap()["status"],
            "unavailable"
        );
        let mut value = model(&maven);
        value[RELEASE] = Value::Null;
        assert_eq!(
            normalize_model(&maven, value).unwrap()["compiler"]["release_enabled"],
            Value::Null
        );
    }

    #[test]
    fn existing_configured_main_source_requires_model_evidence() {
        let (_temp, mut maven) = fixture();
        let mut raw = model(&maven);
        raw[SOURCES] = json!([maven.root.join("src/test/java")]);
        let value = normalize_model(&maven, raw.clone()).unwrap();
        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["maven_nature"], true);
        maven.source_paths[0] = "missing/main".into();
        assert_eq!(normalize_model(&maven, raw).unwrap()["status"], "imported");
    }

    #[test]
    fn declared_entries_must_fit_the_complete_output_budget() {
        let (_temp, mut maven) = fixture();
        maven.declared_dependencies = (0..MAX_CLASSPATH - 1)
            .map(|index| {
                maven
                    .local_repository
                    .join(format!("{index}/{}.jar", "x".repeat(600)))
            })
            .collect();
        assert_eq!(
            normalize_model(&maven, model(&maven)).unwrap_err().code,
            "language_maven_invalid_model"
        );
    }

    #[test]
    fn absent_language_session_cannot_query_maven() {
        let (temp, _maven) = fixture();
        let workspace = Workspace::open(temp.path()).unwrap();
        assert_eq!(
            workspace.maven_model().unwrap_err().code,
            "language_not_running"
        );
    }

    #[test]
    fn rejects_outside_paths_projects_and_wrong_file_kinds() {
        let (temp, maven) = fixture();
        for entry in [
            json!({"kind":1,"path":temp.path().join("outside.jar")}),
            json!({"kind":2,"path":maven.root}),
            json!({"kind":5,"path":"org.eclipse.jdt.launching.JRE_CONTAINER"}),
            json!({"kind":1,"path":maven.root.join("src")}),
            json!({"kind":3,"path":maven.root.join("pom.xml")}),
        ] {
            let mut value = model(&maven);
            value[CLASSPATH] = json!([entry]);
            assert_eq!(
                normalize_model(&maven, value).unwrap_err().code,
                "language_maven_invalid_model"
            );
        }
        let mut value = model(&maven);
        value[SOURCES] = json!([maven.local_repository]);
        assert!(normalize_model(&maven, value).is_err());
    }

    #[test]
    fn metadata_limits_fail_without_partial_imported_results() {
        let (_temp, maven) = fixture();
        let mut value = model(&maven);
        value[SOURCES] = json!(vec![maven.root.clone(); MAX_SOURCES + 1]);
        assert!(normalize_model(&maven, value).is_err());
        let mut value = model(&maven);
        value[CLASSPATH] = json!(vec![json!({"kind":3,"path":maven.root}); MAX_CLASSPATH + 1]);
        assert!(normalize_model(&maven, value).is_err());
        for text in ["x".repeat(MAX_COMPILER + 1), "17\n".into(), "é".into()] {
            let mut value = model(&maven);
            value[SOURCE] = json!(text);
            assert!(normalize_model(&maven, value).is_err());
        }
        let mut value = model(&maven);
        value[SOURCES] = json!([format!("/{}", "x".repeat(MAX_PATH))]);
        assert!(normalize_model(&maven, value).is_err());
        let mut value = model(&maven);
        value["unknown"] = json!("x".repeat(MAX_JSON));
        assert!(normalize_model(&maven, value).is_err());
        assert!(unavailable(&maven)["message"].as_str().unwrap().len() <= 256);
    }

    #[test]
    fn path_spelling_rejects_traversal_authority_stream_and_device_aliases() {
        assert_eq!(
            normalized_spelling(r"C:\work\src", true).unwrap(),
            "C:/work/src"
        );
        for text in [
            r"\\server\share\file",
            r"\\?\C:\work",
            "C:/work/../file",
            "C:/work/./file",
            "C:/work//file",
            "C:/work/file:stream",
            "C:/work/NUL.jar",
            "C:/work/COM1",
            "C:/work/name.",
            "C:/work/name ",
            "file:///C:/work",
            "C:work",
            "/work",
        ] {
            assert!(normalized_spelling(text, true).is_err(), "{text}");
        }
        for text in [
            "//server/path",
            "/work/../file",
            "/work/./file",
            "/work//file",
            "relative",
            "/work/\nfile",
        ] {
            assert!(normalized_spelling(text, false).is_err(), "{text}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn existing_symlink_ancestors_are_rejected_even_for_missing_tails() {
        let (temp, maven) = fixture();
        std::os::unix::fs::symlink(temp.path(), maven.root.join("alias")).unwrap();
        let mut value = model(&maven);
        value[SOURCES] = json!([maven.root.join("alias/missing/source")]);
        assert!(normalize_model(&maven, value).is_err());
    }

    #[test]
    fn query_has_one_fixed_request_and_checks_pom_before_and_after() {
        let (_temp, maven) = fixture();
        let result = query_model(&maven, true, |params, timeout| {
            assert_eq!(timeout, Duration::from_secs(5));
            assert_eq!(params, json!({"command":"java.project.getSettings","arguments":[maven.pom_uri,[NATURES,SOURCES,CLASSPATH,SOURCE,COMPLIANCE,TARGET,RELEASE]]}));
            Ok(model(&maven))
        }).unwrap();
        assert_eq!(result["pom_sha256"], maven.pom_sha256);
        assert_eq!(result["status"], "imported");
        assert_eq!(
            query_model(&maven, false, |_, _| panic!(
                "unsupported session must not request"
            ))
            .unwrap_err()
            .code,
            "language_maven_unsupported"
        );
        assert_eq!(
            query_model(&maven, true, |_, _| Err(())).unwrap()["status"],
            "unavailable"
        );
        let error = query_model(&maven, true, |_, _| {
            fs::write(maven.root.join("pom.xml"), b"changed").unwrap();
            Err(())
        })
        .unwrap_err();
        assert_eq!(error.code, "language_maven_restart_required");
        let error =
            query_model(&maven, true, |_, _| panic!("stale POM must not request")).unwrap_err();
        assert_eq!(error.code, "language_maven_restart_required");
    }
}
