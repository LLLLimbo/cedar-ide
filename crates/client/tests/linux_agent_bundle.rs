//! Nonshipping acceptance for an already verified and extracted Linux bundle.
//! CEDAR_LINUX_BUNDLE_AGENT_BIN must name its absolute cedar-agent executable.
//! Run the single ignored test explicitly; ordinary Linux Local is embedded and
//! cannot verify the extracted executable. This covers local stdio, not SSH,
//! network authentication, GUI behavior, language tools, or deployment.
#![cfg(target_os = "linux")]

use cedar_client::Client;
use cedar_protocol::{
    AgentInfo, GitDiffKind, LanguageQueryKind, Operation, Payload, AGENT_INFO_SCHEMA,
    LANGUAGE_SESSION_CAPABILITIES, PROTOCOL_VERSION, RUN_TASK_CAPABILITIES,
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const ROOT: &str = "workspace café 雪";
const MARKER: &str = ".cedar-linux-bundle-acceptance";
const MARKER_BYTES: &[u8] = b"cedar-linux-bundle-acceptance-v1\n";
const SOURCE: &str = "source preserved.txt";
const SOURCE_BYTES: &[u8] = b"cedar-linux-source-preserved-v1\n";
const OUTSIDE: &str = "outside preserved.txt";
const OUTSIDE_BYTES: &[u8] = b"cedar-linux-outside-preserved-v1\n";
const DIRECTORY: &str = "files café 雪";
const FILE_NAME: &str = "saved file λ.txt";
const FILE: &str = "files café 雪/saved file λ.txt";
const ORIGINAL: &str = "cedar-linux-bundle-original-v1 café 雪\n";
const SAVED: &str = "cedar-linux-bundle-saved-v1 café 雪\n";
const SAVED_QUERY: &str = "cedar-linux-bundle-saved-v1";
const STALE: &str = "cedar-linux-bundle-stale-v1\n";
const CLOSE_BOUND: Duration = Duration::from_secs(5);
const ELAPSED_BOUND: Duration = Duration::from_secs(30);
const REQUEST_LIMIT: usize = 96;
const AGENT_SPAWN_LIMIT: usize = 2;

// Never retain raw environment values, errors, roots, revisions, or content in
// test diagnostics. Every failure emitted by this test is a fixed stage name.
type ProbeResult<T> = Result<T, &'static str>;

struct Budget {
    started: Instant,
    requests: usize,
    spawns: usize,
    reaped: usize,
}

impl Budget {
    fn check(&self) -> ProbeResult<()> {
        if self.started.elapsed() > ELAPSED_BOUND {
            return Err("elapsed_bound");
        }
        Ok(())
    }

    fn request(
        &mut self,
        client: &mut Client,
        operation: Operation,
    ) -> ProbeResult<Result<Payload, String>> {
        self.check()?;
        if self.requests >= REQUEST_LIMIT {
            return Err("client_call_limit");
        }
        self.requests += 1;
        let result = client.request(operation);
        self.check()?;
        Ok(result)
    }

    fn successful(
        &mut self,
        client: &mut Client,
        operation: Operation,
        stage: &'static str,
    ) -> ProbeResult<Payload> {
        self.request(client, operation)?.map_err(|_| stage)
    }
}

// A panic during verification still has one owner that consumes the Client and
// asks its existing owned-child reaper to finish. Error paths use explicit close
// and propagate its acknowledgement failure. Drop is only a failure fallback;
// it never sets a successful cleanup receipt. No new watchdog or process API.
struct OwnedClient(Option<Client>);

impl OwnedClient {
    fn close(mut self) -> ProbeResult<()> {
        self.0
            .take()
            .ok_or("client_owner")?
            .close_and_wait(CLOSE_BOUND)
            .map_err(|_| "client_reaped")
    }
}

impl Drop for OwnedClient {
    fn drop(&mut self) {
        if let Some(client) = self.0.take() {
            let _ = client.close_and_wait(CLOSE_BOUND);
        }
    }
}

fn with_client<T>(
    agent: &Path,
    root: &Path,
    budget: &mut Budget,
    verify: impl FnOnce(&mut Client, &mut Budget) -> ProbeResult<T>,
) -> ProbeResult<T> {
    budget.check()?;
    if budget.spawns >= AGENT_SPAWN_LIMIT {
        return Err("agent_spawn_limit");
    }
    budget.spawns += 1;
    // Deliberately use the ordinary process transport with execution trust off.
    // The wrapper supplies the verified extraction path; there is no fallback.
    let client = Client::spawn_agent(agent, root, false).map_err(|_| "connect")?;
    let mut owned = OwnedClient(Some(client));
    let verified = verify(owned.0.as_mut().ok_or("client_owner")?, budget);
    owned.close()?;
    budget.reaped += 1;
    budget.check()?;
    verified
}

fn expected_capabilities() -> Vec<&'static str> {
    // Use the shared lifecycle groups plus the explicit Linux platform set.
    // The isolated Linux agent includes typed Java; Maven stays Windows-only.
    let mut expected = vec![
        "list",
        "read",
        "write",
        "search",
        "git_status",
        "git_changes",
        "git_diff",
        "run",
        "language_query",
        "language_format",
        "language_references",
        "language_document_symbols",
        "language_workspace_symbols",
        "language_resolve_uri",
        "language_resolve_completion",
        "language_start_java",
        "language_start_java_begin",
        "language_start_java_poll",
        "language_start_java_cancel",
        "java_diagnostics_refresh",
        "language_organize_java_imports",
        "language_java_implementations",
    ];
    expected.extend_from_slice(RUN_TASK_CAPABILITIES);
    expected.extend_from_slice(LANGUAGE_SESSION_CAPABILITIES);
    expected.sort_unstable();
    expected
}

fn metadata(client: &Client, root: &Path) -> ProbeResult<AgentInfo> {
    let Payload::Hello {
        protocol,
        root: reported,
        agent: Some(info),
    } = client.handshake()
    else {
        return Err("metadata");
    };
    let expected = expected_capabilities();
    if PROTOCOL_VERSION != 4
        || AGENT_INFO_SCHEMA != 1
        || *protocol != 4
        || info.schema != 1
        || reported != root.to_str().ok_or("root_utf8")?
        || info.validate().is_err()
        || info.version != env!("CARGO_PKG_VERSION")
        || info.os != "linux"
        || info.arch != "x86_64"
        || !expected.windows(2).all(|pair| pair[0] < pair[1])
        || info.capabilities != expected
    {
        return Err("metadata");
    }
    Ok(info.clone())
}

fn read_expected(client: &mut Client, budget: &mut Budget, expected: &str) -> ProbeResult<String> {
    let Payload::File {
        path,
        text,
        revision,
    } = budget.successful(client, Operation::Read { path: FILE.into() }, "read")?
    else {
        return Err("read");
    };
    if path != FILE || text != expected || revision.is_empty() {
        return Err("read");
    }
    Ok(revision)
}

fn refused_and_usable(
    client: &mut Client,
    budget: &mut Budget,
    operation: Operation,
    prefix: &'static str,
    revision: &str,
) -> ProbeResult<()> {
    if !matches!(budget.request(client, operation)?, Err(error) if error.starts_with(prefix)) {
        return Err("operation_refused");
    }
    // Hello is cached by Client; an actual Read proves the process still serves
    // requests and the last successful file revision survives every refusal.
    if !client.is_connected() || read_expected(client, budget, SAVED)? != revision {
        return Err("usable_after_refusal");
    }
    Ok(())
}

fn execution_operations() -> Vec<Operation> {
    // Empty program locations cannot identify an executable even if trust
    // checking regresses. Exact run_disabled proves refusal precedes validation.
    vec![
        Operation::Run {
            program: String::new(),
            args: Vec::new(),
            timeout_secs: 1,
        },
        Operation::RunStart {
            program: String::new(),
            args: Vec::new(),
            timeout_secs: 1,
        },
        Operation::RunPoll { task_id: 1 },
        Operation::RunCancel { task_id: 1 },
        Operation::GitStatus,
        Operation::GitChanges {
            git_executable: String::new(),
        },
        Operation::GitDiff {
            git_executable: String::new(),
            path: FILE.into(),
            kind: GitDiffKind::Unstaged,
        },
        Operation::LanguageStart {
            program: String::new(),
            args: Vec::new(),
        },
        Operation::LanguageOpen {
            path: FILE.into(),
            language_id: "plaintext".into(),
            version: 1,
            text: SAVED.into(),
        },
        Operation::LanguageChange {
            path: FILE.into(),
            version: 2,
            text: STALE.into(),
        },
        Operation::LanguageClose { path: FILE.into() },
        Operation::LanguageQuery {
            path: FILE.into(),
            line: 0,
            character: 0,
            kind: LanguageQueryKind::Hover,
        },
        Operation::LanguageFormat {
            path: FILE.into(),
            version: 1,
            tab_size: 4,
            insert_spaces: true,
        },
        Operation::LanguageReferences {
            path: FILE.into(),
            line: 0,
            character: 0,
            include_declaration: false,
        },
        Operation::LanguageDocumentSymbols { path: FILE.into() },
        Operation::LanguageWorkspaceSymbols {
            query: "NeverLaunched".into(),
        },
        Operation::LanguageResolveUri { uri: String::new() },
        Operation::LanguageResolveCompletion {
            item: serde_json::json!({}),
        },
        Operation::LanguageEvents,
        Operation::LanguageStop,
    ]
}

fn typed_java_operations() -> Vec<Operation> {
    // Empty paths cannot name a Java executable, distribution or data root.
    // Advertised requests must reach run_disabled before path validation.
    vec![
        Operation::LanguageStartJava {
            java_executable: String::new(),
            distribution: String::new(),
            data_directory: String::new(),
        },
        Operation::LanguageStartJavaBegin {
            java_executable: String::new(),
            distribution: String::new(),
            data_directory: String::new(),
        },
        Operation::LanguageStartJavaPoll { startup_id: 1 },
        Operation::LanguageStartJavaCancel { startup_id: 1 },
        Operation::LanguageRefreshJavaDiagnostics {
            path: FILE.into(),
            version: 1,
        },
        Operation::LanguageOrganizeJavaImports {
            path: FILE.into(),
            version: 1,
        },
        Operation::LanguageJavaImplementations {
            path: FILE.into(),
            version: 1,
            line: 0,
            character: 0,
        },
    ]
}

fn unsupported_maven_operations() -> Vec<Operation> {
    // The normal Client rejects unadvertised Maven before sending a request.
    // These empty locations remain safe if capability checking regresses.
    vec![
        Operation::LanguageStartJavaMavenBegin {
            java_executable: String::new(),
            distribution: String::new(),
            data_directory: String::new(),
            local_repository: String::new(),
        },
        Operation::LanguageMavenModel,
        Operation::LanguageMavenDependencies {
            startup_id: 1,
            pom_sha256: "a".repeat(64),
        },
    ]
}

fn verify_initial(
    client: &mut Client,
    root: &Path,
    budget: &mut Budget,
) -> ProbeResult<(AgentInfo, String)> {
    let info = metadata(client, root)?;
    for (path, name, relative, directory) in [
        (".", DIRECTORY, DIRECTORY, true),
        (DIRECTORY, FILE_NAME, FILE, false),
    ] {
        let Payload::Entries { entries } =
            budget.successful(client, Operation::List { path: path.into() }, "list")?
        else {
            return Err("list");
        };
        if !entries
            .iter()
            .any(|entry| entry.name == name && entry.path == relative && entry.is_dir == directory)
        {
            return Err("list");
        }
    }
    let original_revision = read_expected(client, budget, ORIGINAL)?;
    let Payload::Written {
        revision: saved_revision,
    } = budget.successful(
        client,
        Operation::Write {
            path: FILE.into(),
            text: SAVED.into(),
            expected_revision: Some(original_revision.clone()),
        },
        "conditional_write",
    )?
    else {
        return Err("conditional_write");
    };
    if saved_revision.is_empty()
        || saved_revision == original_revision
        || read_expected(client, budget, SAVED)? != saved_revision
    {
        return Err("readback");
    }
    let Payload::Matches { matches, truncated } = budget.successful(
        client,
        Operation::Search {
            query: SAVED_QUERY.into(),
            limit: 10,
        },
        "search",
    )?
    else {
        return Err("search");
    };
    if truncated
        || matches.len() != 1
        || !matches.first().is_some_and(|found| {
            found.path == FILE && found.line == 1 && found.text == SAVED.trim_end()
        })
    {
        return Err("search");
    }
    refused_and_usable(
        client,
        budget,
        Operation::Write {
            path: FILE.into(),
            text: STALE.into(),
            expected_revision: Some(original_revision),
        },
        "conflict:",
        &saved_revision,
    )?;
    // The only attempted outside write names our own disposable sibling file.
    // No request references a user's workspace, home, checkout, or other data.
    let escape = format!("../{OUTSIDE}");
    for operation in [
        Operation::List { path: "..".into() },
        Operation::Read {
            path: escape.clone(),
        },
        Operation::Write {
            path: escape,
            text: STALE.into(),
            expected_revision: None,
        },
    ] {
        refused_and_usable(client, budget, operation, "invalid_path:", &saved_revision)?;
    }
    for operation in execution_operations() {
        refused_and_usable(client, budget, operation, "run_disabled:", &saved_revision)?;
    }
    for operation in typed_java_operations() {
        refused_and_usable(client, budget, operation, "run_disabled:", &saved_revision)?;
    }
    for operation in unsupported_maven_operations() {
        refused_and_usable(
            client,
            budget,
            operation,
            "unsupported_operation:",
            &saved_revision,
        )?;
    }
    let Payload::Hello {
        protocol,
        root: reported,
        agent: Some(after),
    } = budget.successful(client, Operation::Hello, "metadata_stable")?
    else {
        return Err("metadata_stable");
    };
    if protocol != 4 || reported != root.to_str().ok_or("root_utf8")? || after != info {
        return Err("metadata_stable");
    }
    Ok((info, saved_revision))
}

#[derive(PartialEq, Eq)]
enum SnapshotEntry {
    Directory,
    File(Vec<u8>),
}

fn snapshot(base: &Path) -> ProbeResult<BTreeMap<PathBuf, SnapshotEntry>> {
    fn visit(
        base: &Path,
        relative: &Path,
        entries: &mut BTreeMap<PathBuf, SnapshotEntry>,
    ) -> ProbeResult<()> {
        if entries.len() >= 16 || relative.components().count() > 3 {
            return Err("fixture_snapshot_bound");
        }
        let path = base.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|_| "fixture_snapshot")?;
        if metadata.file_type().is_symlink() {
            return Err("fixture_snapshot");
        }
        if metadata.is_file() && metadata.len() <= 4096 {
            entries.insert(
                relative.into(),
                SnapshotEntry::File(fs::read(&path).map_err(|_| "fixture_snapshot")?),
            );
        } else if metadata.is_dir() {
            entries.insert(relative.into(), SnapshotEntry::Directory);
            for entry in fs::read_dir(&path).map_err(|_| "fixture_snapshot")? {
                let entry = entry.map_err(|_| "fixture_snapshot")?;
                visit(base, &relative.join(entry.file_name()), entries)?;
            }
        } else {
            return Err("fixture_snapshot");
        }
        Ok(())
    }
    let mut entries = BTreeMap::new();
    visit(base, Path::new(""), &mut entries)?;
    Ok(entries)
}

fn verify() -> ProbeResult<serde_json::Value> {
    let mut budget = Budget {
        started: Instant::now(),
        requests: 0,
        spawns: 0,
        reaped: 0,
    };
    if std::env::consts::ARCH != "x86_64" {
        return Err("host_architecture");
    }
    let agent =
        PathBuf::from(std::env::var_os("CEDAR_LINUX_BUNDLE_AGENT_BIN").ok_or("agent_environment")?);
    let agent_metadata = fs::symlink_metadata(&agent).map_err(|_| "agent_path")?;
    if !agent.is_absolute()
        || agent.file_name().and_then(|name| name.to_str()) != Some("cedar-agent")
        || !agent_metadata.is_file()
        || agent_metadata.file_type().is_symlink()
        || agent_metadata.permissions().mode() & 0o111 == 0
    {
        return Err("agent_path");
    }
    let fixture = tempfile::Builder::new()
        .prefix("cedar linux bundle 验证 ")
        .tempdir()
        .map_err(|_| "fixture_create")?;
    let root = fixture.path().join(ROOT);
    fs::create_dir_all(root.join(DIRECTORY)).map_err(|_| "fixture_create")?;
    // SOURCE, MARKER and OUTSIDE must remain byte-for-byte unchanged. Only FILE
    // is deliberately edited, from ORIGINAL to SAVED, through conditional Write.
    for (path, bytes) in [
        (root.join(MARKER), MARKER_BYTES),
        (root.join(SOURCE), SOURCE_BYTES),
        (fixture.path().join(OUTSIDE), OUTSIDE_BYTES),
        (root.join(FILE), ORIGINAL.as_bytes()),
    ] {
        fs::write(path, bytes).map_err(|_| "fixture_create")?;
    }
    let root = root.canonicalize().map_err(|_| "fixture_root")?;
    let mut expected = snapshot(fixture.path())?;
    let deliberate_change = Path::new(ROOT).join(FILE);
    if expected.get(&deliberate_change) != Some(&SnapshotEntry::File(ORIGINAL.as_bytes().to_vec()))
    {
        return Err("fixture_original");
    }
    expected.insert(
        deliberate_change,
        SnapshotEntry::File(SAVED.as_bytes().to_vec()),
    );
    let (info, saved_revision) = with_client(&agent, &root, &mut budget, |client, budget| {
        verify_initial(client, &root, budget)
    })?;
    if snapshot(fixture.path())? != expected {
        return Err("fixture_preserved");
    }
    with_client(&agent, &root, &mut budget, |client, budget| {
        if metadata(client, &root)? != info
            || read_expected(client, budget, SAVED)? != saved_revision
        {
            return Err("reconnect");
        }
        Ok(())
    })?;
    if snapshot(fixture.path())? != expected {
        return Err("fixture_preserved");
    }
    fixture.close().map_err(|_| "fixture_removed")?;
    budget.check()?;
    if budget.spawns != AGENT_SPAWN_LIMIT || budget.reaped != budget.spawns {
        return Err("client_reaped");
    }
    // No duplicated hardcoded capability count. Apart from the fixed kind and
    // success tags, receipt values are booleans or bounded numbers: no host,
    // path, version string, or diagnostic.
    Ok(serde_json::json!({
        "kind": "cedar_linux_agent_bundle_probe",
        "schema_version": 1,
        "status": "success",
        "protocol_version": 4,
        "agent_info_schema": 1,
        "linux_x86_64": true,
        "extracted_agent_stdio": true,
        "package_version_matches": true,
        "capabilities_exact": true,
        "capability_count": info.capabilities.len(),
        "trust_off": true,
        "list_verified": true,
        "read_verified": true,
        "conditional_write_verified": true,
        "readback_verified": true,
        "search_verified": true,
        "stale_write_rejected": true,
        "root_escape_rejected": true,
        "task_operations_rejected": true,
        "language_operations_rejected": true,
        "git_operations_rejected": true,
        "typed_java_advertised_maven_unadvertised": true,
        "errors_leave_client_usable": true,
        "preserved_fixture_unchanged": true,
        "only_expected_file_changed": true,
        "reconnect_saved_bytes": true,
        "initial_client_reaped": true,
        "reconnect_client_reaped": true,
        "fixture_removed": true,
        "agent_processes_spawned": budget.spawns,
        "explicit_client_calls": budget.requests,
        "elapsed_ms": budget.started.elapsed().as_millis(),
        "elapsed_bound_ms": ELAPSED_BOUND.as_millis(),
        "client_call_limit": REQUEST_LIMIT,
        "agent_spawn_limit": AGENT_SPAWN_LIMIT,
        "close_bound_ms": CLOSE_BOUND.as_millis(),
    }))
}

#[test]
#[ignore = "requires an explicitly verified extracted Linux x86_64 bundle agent"]
fn extracted_linux_agent_stdio_acceptance() {
    match verify() {
        // Leading newline keeps the one JSON receipt separate from libtest's
        // progress prefix under --nocapture. The wrapper validates its schema.
        Ok(receipt) => println!("\n{receipt}"),
        Err(stage) => panic!("linux_bundle_acceptance:{stage}"),
    }
}
