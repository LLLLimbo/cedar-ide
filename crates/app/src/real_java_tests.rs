//! Opt-in real JDT LS → workspace protocol → frontend transaction/undo verification.
//! CEDAR_JDTLS_HOME=/path/to/jdtls cargo test -p cedar-app real_java_completion -- --ignored --nocapture
use super::{Action, ActionKind, QueryContext};
use crate::{completion, editor_state, language_results, model::Document};
use cedar_client::{Client, ConnectionSpec};
use cedar_protocol::{LanguageQueryKind, Operation, Payload};
use serde_json::{json, Value};
use std::{
    error::Error,
    path::PathBuf,
    time::{Duration, Instant},
};

#[cfg(feature = "windows-language-validation")]
#[path = "real_java_acceptance_tests.rs"]
mod acceptance;

fn language(client: &mut Client, op: Operation) -> Result<Value, Box<dyn Error>> {
    match client.request(op)? {
        Payload::Language { value } => Ok(value),
        _ => Err("Unexpected non-language response".into()),
    }
}

#[test]
#[ignore = "requires explicit CEDAR_JDTLS_HOME and an installed Java 21+ runtime"]
fn real_java_completion_import_and_editor_undo() -> Result<(), Box<dyn Error>> {
    let jdtls = PathBuf::from(std::env::var("CEDAR_JDTLS_HOME")?).canonicalize()?;
    let java = std::env::var("CEDAR_JAVA").unwrap_or_else(|_| "java".into());
    let jars: Vec<_> = std::fs::read_dir(jdtls.join("plugins"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("org.eclipse.equinox.launcher_")
            }) && path.extension().is_some_and(|extension| extension == "jar")
        })
        .collect();
    assert_eq!(jars.len(), 1);
    let temp = tempfile::Builder::new()
        .prefix("cedar-editor-java-")
        .tempdir()?;
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::create_dir_all(project.join(".settings"))?;
    std::fs::write(project.join(".project"), "<?xml version=\"1.0\"?><projectDescription><name>cedar-editor-smoke</name><projects/><buildSpec><buildCommand><name>org.eclipse.jdt.core.javabuilder</name><arguments/></buildCommand></buildSpec><natures><nature>org.eclipse.jdt.core.javanature</nature></natures></projectDescription>")?;
    std::fs::write(project.join(".classpath"), "<?xml version=\"1.0\"?><classpath><classpathentry kind=\"src\" path=\"src\"/><classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/><classpathentry kind=\"output\" path=\"bin\"/></classpath>")?;
    std::fs::write(project.join(".settings/org.eclipse.jdt.core.prefs"), "eclipse.preferences.version=1\norg.eclipse.jdt.core.compiler.codegen.targetPlatform=21\norg.eclipse.jdt.core.compiler.compliance=21\norg.eclipse.jdt.core.compiler.source=21\n")?;
    let source = "public class Main {\n    public static void main(String[] args) {\n        String greeting = \"Hello Cedar\";\n        System.out.println(greeting);\n        int broken = \"oops\";\n    }\n}\n";
    std::fs::write(project.join("src/Main.java"), source)?;
    let mut args: Vec<String> = [
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
    .map(str::to_owned)
    .collect();
    args.push(jars[0].to_string_lossy().into_owned());
    args.push("-configuration".into());
    args.push(
        jdtls
            .join(if cfg!(target_os = "macos") {
                "config_mac"
            } else {
                "config_linux"
            })
            .to_string_lossy()
            .into_owned(),
    );
    args.push("-data".into());
    args.push(temp.path().join("jdt-data").to_string_lossy().into_owned());
    let mut client = Client::connect(ConnectionSpec::Local {
        root: project.clone(),
        allow_run: true,
    })?;
    let started = Instant::now();
    let initialized = language(
        &mut client,
        Operation::LanguageStart {
            program: java,
            args,
        },
    )?;
    println!(
        "{}",
        json!({"kind":"editor_initialize", "server":initialized["initialize"]["serverInfo"], "elapsed_ms":started.elapsed().as_millis()})
    );
    let opened = language(
        &mut client,
        Operation::LanguageOpen {
            path: "src/Main.java".into(),
            language_id: "java".into(),
            version: 1,
            text: source.into(),
        },
    )?;
    let uri = opened["opened"]
        .as_str()
        .ok_or("Missing opened URI")?
        .to_owned();
    let mut diagnostics = language_results::Diagnostics::default();
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline && diagnostics.len() == 0 {
        let events = language(&mut client, Operation::LanguageEvents)?;
        for event in events["events"].as_array().ok_or("Missing events")? {
            if event["type"] == "diagnostics" && event["value"]["uri"] == uri {
                diagnostics.apply(&event["value"])?;
            }
        }
        if diagnostics.len() == 0 {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    assert!(
        diagnostics.len() > 0,
        "Real semantic diagnostics were not received"
    );
    let byte = source.rfind("greeting").ok_or("No fixture symbol")? + 3;
    let cursor = completion::byte_to_position(source, byte)?;
    let definitions = language(
        &mut client,
        Operation::LanguageQuery {
            path: "src/Main.java".into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Definition,
        },
    )?;
    let locations = language_results::parse_definitions(&definitions)?;
    let location = locations.first().ok_or("No actual definition")?;
    let resolved_path = language(
        &mut client,
        Operation::LanguageResolveUri {
            uri: location.uri.clone(),
        },
    )?;
    assert_eq!(resolved_path["path"], "src/Main.java");
    let response = language(
        &mut client,
        Operation::LanguageQuery {
            path: "src/Main.java".into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Completion,
        },
    )?;
    let result = completion::parse_completion_result(&response)?;
    let candidate = result
        .candidates
        .into_iter()
        .find(|candidate| candidate.label.starts_with("GregorianCalendar"))
        .ok_or("Missing real GregorianCalendar completion")?;
    assert!(
        candidate.disabled_reason.is_none(),
        "Candidate unexpectedly disabled: {:?}",
        candidate.disabled_reason
    );
    let original_item = candidate.item.clone();
    let resolved = language(
        &mut client,
        Operation::LanguageResolveCompletion {
            item: candidate.item,
        },
    )?;
    let applied = completion::apply_completion(source, cursor, &resolved)?;
    assert!(applied
        .text
        .starts_with("import java.util.GregorianCalendar;\n"));
    assert!(applied
        .text
        .contains("System.out.println(GregorianCalendar);"));
    assert_eq!(applied.edit_count, 2);
    assert!(applied.skipped_advisory);
    let mut app = crate::CedarApp::empty();
    app.language.running = true;
    app.language.session = 3;
    app.language.acceptance_sequence = 1;
    let mut doc = Document::new(
        1,
        "src/Main.java".into(),
        source.into(),
        "disk-revision".into(),
    );
    doc.cursor = crate::model::cursor_location(source, source[..byte].chars().count());
    app.documents.push(doc);
    app.active_document = Some(1);
    app.apply_language_action(
        Action {
            session: 3,
            kind: ActionKind::ResolveCompletion {
                context: QueryContext {
                    session: 3,
                    document: 1,
                    edit_version: 0,
                    source: source.into(),
                    cursor,
                },
                original: original_item,
                acceptance: 1,
            },
        },
        resolved,
    );
    assert!(
        app.error.is_none(),
        "Frontend rejected real resolve: {:?}",
        app.error
    );
    let doc = &mut app.documents[0];
    assert_eq!(doc.text, applied.text);
    assert!(doc.dirty());
    let state = editor_state::load(&app.editor_ctx, doc);
    let mut undoer = state.undoer();
    let after = (state.cursor.char_range().unwrap(), doc.text.clone());
    let before = undoer
        .undo(&after)
        .ok_or("Completion missing undo state")?
        .clone();
    assert_eq!(before.1, source);
    let redone = undoer
        .redo(&before)
        .ok_or("Completion missing redo state")?
        .clone();
    assert_eq!(redone.1, applied.text);
    assert_eq!(
        std::fs::read_to_string(project.join("src/Main.java"))?,
        source,
        "Completion must not save implicitly"
    );
    language(
        &mut client,
        Operation::LanguageChange {
            path: "src/Main.java".into(),
            version: 2,
            text: applied.text,
        },
    )?;
    language(
        &mut client,
        Operation::LanguageChange {
            path: "src/Main.java".into(),
            version: 3,
            text: before.1,
        },
    )?;
    language(
        &mut client,
        Operation::LanguageClose {
            path: "src/Main.java".into(),
        },
    )?;
    language(&mut client, Operation::LanguageStop)?;
    println!(
        "{}",
        json!({"kind":"editor_completion_pass", "elapsed_ms":started.elapsed().as_millis(), "checks":["real_semantic_diagnostics","real_definition","agent_confined_uri","real_completion","lazy_import_resolve","atomic_primary_plus_import","frontend_resolve_identity_and_snapshot_guards","editor_one_step_undo","editor_redo","disk_unchanged","didChange_after_apply_and_undo","server_shutdown"], "server_commands_executed":false, "advisory_callback_skipped":true, "os_window_interaction":false})
    );
    Ok(())
}
