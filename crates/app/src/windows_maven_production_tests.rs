//! Finite, opt-in native Windows Maven acceptance through the shipping agent and
//! normal capability-enforcing Client. This tests one generated leaf with its
//! dependency present and one fresh leaf with that dependency missing. It does
//! not authorize user projects, run Maven goals, establish GUI trust, or claim
//! network isolation: JDT can still request public Gradle version metadata.
use super::*;
use crate::{
    editor_state,
    java_language::{JavaStopOutcome, StopReason, StopStatus},
    model::Document,
};
use cedar_client::Client;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, os::windows::fs::MetadataExt};
use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;

const MODEL_BUDGET: Duration = Duration::from_secs(60);
const MODEL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_MODEL_QUERIES: u16 = 240;
const SEMANTIC_BUDGET: Duration = Duration::from_secs(20);
const PAIR_BUDGET: Duration = Duration::from_secs(360);
// Test observation only: the normal Client already allows 75 seconds for Stop.
// Allow another 15 seconds for verification/reaping, without changing the
// production shutdown grace, RPC timeout, or the 360-second pair watchdog.
const CLEANUP_BUDGET: Duration = Duration::from_secs(90);
const CLIENT_REAP_BUDGET: Duration = Duration::from_secs(10);
const CACHE_FILES: usize = 83;
const SOURCE_FILE: &str = "source-java/demo/Main.java";
const DEPENDENCY_DIRECTORY: &str = "dev/cedar/fixture/arithmetic/1.0.0";
const DEPENDENCY_JAR: &str = "dev/cedar/fixture/arithmetic/1.0.0/arithmetic-1.0.0.jar";
const DEPENDENCY_POM: &str = "dev/cedar/fixture/arithmetic/1.0.0/arithmetic-1.0.0.pom";
const DEPENDENCY_GAV: &str = "dev.cedar.fixture:arithmetic:jar:1.0.0";
const JAR_SHA256: &str = "82579c654968c77f0bd3d04c28a22b24396c35270ce76d015807410438952b5d";
const SOURCE_SHA256: &str = "5dda0de22c2184b1420be8e68f8a37e9165b59658d5c5cbf9fe2ee770a1003e5";
const POM_SHA256: &str = "c13116c2a4d7dd73f28f604480b6aad3ce818a11db526b7f61737c1c3864b65b";
const FIXTURE_SOURCE: &str = "package demo;\nimport cedar.fixture.Arithmetic;\npublic class Main {\n    public static void main(String[] args) {\n        int known = Arithmetic.answer();\n        String invalid = known;\n        System.out.println(invalid);\n    }\n}\n";
const FIXTURE_POM: &str = "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>dev.cedar.feasibility</groupId>\n  <artifactId>leaf-project</artifactId>\n  <version>1.0.0</version>\n  <properties>\n    <maven.compiler.release>17</maven.compiler.release>\n    <maven.compiler.source>17</maven.compiler.source>\n    <maven.compiler.target>17</maven.compiler.target>\n    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>\n  </properties>\n  <build><sourceDirectory>source-java</sourceDirectory></build>\n  <dependencies>\n    <dependency>\n      <groupId>dev.cedar.fixture</groupId>\n      <artifactId>arithmetic</artifactId>\n      <version>1.0.0</version>\n    </dependency>\n  </dependencies>\n</project>\n";
const ARTIFACT_POM: &str = "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>dev.cedar.fixture</groupId>\n  <artifactId>arithmetic</artifactId>\n  <version>1.0.0</version>\n</project>\n";

// Deterministic stored ZIP (1980 timestamp, one Java 17 class, no manifest or
// executable entry point), generated solely from this reviewed source:
// package cedar.fixture;
// public final class Arithmetic {
//     public static int answer() { return 42; }
// }
// The JDK 21 compiler used -proc:none -source 17 -target 17, producing classfile
// version 61 (264 bytes); this does not claim a Java 17 platform API check.
// Acceptance decodes and hashes the sealed 422-byte JAR; it never invokes
// javac, Maven, a wrapper, or a build.
const JAR_HEX: &str = concat!(
    "504b0304140000000000000021003be39e7508010000080100001e00000063656461722f666978747572652f41726974686d657469632e636c617373",
    "cafebabe0000003d000f0a000200030700040c000500060100106a6176612f6c616e672f4f626a6563740100063c696e69743e01000328295607000801001863656461722f666978747572652f41726974686d65746963010004436f646501000f4c696e654e756d6265725461626c65010006616e7377657201000328294901000a536f7572636546696c6501000f41726974686d657469632e6a617661003100070002000000000002000100050006000100090000001d00010001000000052ab70001b100000001000a000000060001000000020009000b000c000100090000001b0001000000000003102aac00000001000a000000060001000000030001000d00000002000e",
    "504b01021403140000000000000021003be39e7508010000080100001e0000000000000000000000a4810000000063656461722f666978747572652f41726974686d657469632e636c617373504b050600000000010001004c000000440100000000"
);

#[derive(Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CaseKind {
    #[default]
    Present,
    Missing,
}
impl CaseKind {
    fn present(self) -> bool {
        matches!(self, Self::Present)
    }
}

fn case_budget(kind: CaseKind) -> Duration {
    START_TIMEOUT + MODEL_BUDGET + semantic_budget(kind) + CLEANUP_BUDGET
}

fn semantic_budget(kind: CaseKind) -> Duration {
    if kind.present() {
        SEMANTIC_BUDGET
    } else {
        Duration::ZERO
    }
}

fn budget_admits(now: Instant, deadline: Instant, work: Duration, reserve: Duration) -> bool {
    now < deadline && deadline.saturating_duration_since(now) >= work + reserve
}

fn admit_work(deadline: Instant, work: Duration, reserve: Duration) -> CheckResult<()> {
    require(
        budget_admits(Instant::now(), deadline, work, reserve),
        "Maven pair has insufficient remaining observation and cleanup budget",
    )
}

fn bounded_cleanup_deadline(now: Instant, pair_deadline: Instant) -> Instant {
    (now + CLEANUP_BUDGET).min(pair_deadline)
}

fn prior_cleanup_verified(record: &CaseEvidence) -> bool {
    !record.cleanup_failed
        && record.stop_outcome_verified
        && record.cleanup_joined
        && record.root_handle_signaled
        && record.root_exit_code.is_some()
        && record.client_reaped
        && record.synthetic_root_removed
}
#[derive(Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    #[default]
    Setup,
    Trust,
    Startup,
    Model,
    Semantics,
    PomChange,
    Stop,
    RootExit,
    ClientExit,
    FixtureCleanup,
    None,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ModelStatus {
    #[default]
    Unavailable,
    Imported,
    Unresolved,
}

// Only fixed tags, booleans, bounded counters and native exit status leave this
// test. No event payloads, source text, private paths or server messages appear
// in the receipt. The collector must independently require both cases.
#[derive(Default, serde::Serialize)]
struct CaseEvidence {
    case: CaseKind,
    failure_stage: Stage,
    model_status: ModelStatus,
    model_queries: u16,
    java_capabilities: bool,
    generic_start_rejected: bool,
    untrusted_start_rejected: bool,
    model_without_session_rejected: bool,
    async_start_begin_acknowledged: bool,
    async_start_read_while_starting: bool,
    async_start_ready: bool,
    root_identity_verified: bool,
    root_observed_live: bool,
    maven_nature: bool,
    custom_source: bool,
    compiler_17: bool,
    exact_dependency_reference: bool,
    unexpected_dependency_references: u16,
    offline_pom_diagnostic: bool,
    hover: bool,
    completion: bool,
    deliberate_type_diagnostic: bool,
    dirty_change_acknowledged: bool,
    no_autosave: bool,
    stale_startup_rejected: bool,
    changed_pom_restart_required: bool,
    source_unchanged: bool,
    pom_expected: bool,
    repository_inputs_unchanged: bool,
    dependency_jar_present_before: bool,
    dependency_jar_present_after: bool,
    dependency_pom_present_before: bool,
    dependency_pom_present_after: bool,
    generated_metadata_files: u16,
    foreign_repository_files: u16,
    lifecycle_metadata_files: u8,
    lifecycle_metadata_mask: u8,
    generated_data_files: u32,
    generated_data_bytes: u64,
    generated_project_files: u16,
    generated_project_bytes: u64,
    stop_status: Option<StopStatus>,
    stop_reason: Option<StopReason>,
    cleanup_joined: bool,
    shutdown_response_received: bool,
    exit_frame_completed: bool,
    stop_outcome_verified: bool,
    root_handle_signaled: bool,
    root_exit_code: Option<u32>,
    model_after_stop_rejected: bool,
    client_reaped: bool,
    synthetic_root_removed: bool,
    primary_failed: bool,
    cleanup_failed: bool,
    success: bool,
}

#[derive(serde::Serialize)]
struct MavenEvidence {
    schema_version: u8,
    kind: &'static str,
    route: &'static str,
    pair_count: u8,
    cache_input_files: u16,
    cache_input_unchanged: bool,
    fixture_inputs_verified: bool,
    source_sha256: &'static str,
    pom_sha256: &'static str,
    dependency_jar_sha256: &'static str,
    present: CaseEvidence,
    missing: CaseEvidence,
    elapsed_ms: u32,
    elapsed_saturated: bool,
    primary_failed: bool,
    cleanup_failed: bool,
    success: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct Seal {
    bytes: u64,
    sha256: String,
}
type Inventory = BTreeMap<PathBuf, Seal>;
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn snapshot(root: &Path) -> CheckResult<Inventory> {
    let mut directories = vec![root.to_path_buf()];
    let mut inventory = Inventory::new();
    let mut entries = 0usize;
    let mut bytes = 0u64;
    while let Some(directory) = directories.pop() {
        for entry in io(fs::read_dir(directory))? {
            entries += 1;
            require(
                entries <= 4096,
                "Maven cache inventory exceeded its entry bound",
            )?;
            let entry = io(entry)?;
            let path = entry.path();
            let metadata = io(fs::symlink_metadata(&path))?;
            require(
                !metadata.file_type().is_symlink() && metadata.file_attributes() & 0x400 == 0,
                "Maven cache inventory contains a link or reparse point",
            )?;
            if metadata.is_dir() {
                directories.push(path);
                continue;
            }
            require(
                metadata.is_file() && metadata.len() <= 8 * 1024 * 1024,
                "Maven cache input is not a bounded ordinary file",
            )?;
            bytes += metadata.len();
            require(
                bytes <= 16 * 1024 * 1024 && inventory.len() < 1024,
                "Maven cache inventory exceeded its byte or file bound",
            )?;
            let contents = io(fs::read(&path))?;
            require(
                contents.len() as u64 == metadata.len(),
                "Maven cache changed while reading",
            )?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "Maven cache escaped its root")?
                .to_path_buf();
            inventory.insert(
                relative,
                Seal {
                    bytes: metadata.len(),
                    sha256: hash(&contents),
                },
            );
        }
    }
    Ok(inventory)
}

fn absent(path: &Path) -> CheckResult<bool> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err("Maven fixture absence could not be verified".into()),
        Ok(_) => Ok(false),
    }
}
fn sealed_jar() -> CheckResult<Vec<u8>> {
    let bytes = JAR_HEX
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|_| "invalid sealed fixture encoding")?;
            u8::from_str_radix(pair, 16).map_err(|_| "invalid sealed fixture byte")
        })
        .collect::<Result<Vec<_>, _>>()?;
    require(
        bytes.len() == 422 && hash(&bytes) == JAR_SHA256,
        "sealed generated JAR hash mismatch",
    )?;
    Ok(bytes)
}
fn language_value(client: &mut Client, op: Operation) -> CheckResult<Value> {
    match client.request(op)? {
        Payload::Language { value } => Ok(value),
        _ => Err("Maven acceptance received a non-language response".into()),
    }
}
fn rejected(client: &mut Client, op: Operation, code: &str) -> CheckResult<()> {
    require(
        client.request(op).is_err_and(|error| error.contains(code)),
        "Maven acceptance did not receive the expected typed rejection",
    )
}

fn verify_image(process: &RootObservation, java: &Path) -> CheckResult<()> {
    let mut image = vec![0u16; 32_768];
    let mut length = image.len() as u32;
    // SAFETY: The common Java fixture retained this owned root's query-only
    // process handle; this observes its exact image without searching or killing.
    require(
        unsafe {
            QueryFullProcessImageNameW(
                process.handle.as_raw_handle(),
                0,
                image.as_mut_ptr(),
                &mut length,
            )
        } != 0,
        "Maven root image query failed",
    )?;
    let image =
        String::from_utf16(&image[..length as usize]).map_err(|_| "invalid Maven root image")?;
    require(
        ordinary_path(Path::new(&image))? == ordinary_path(java)?
            && process.created != 0
            && process.live()?,
        "Maven root image or live identity mismatch",
    )
}

fn capabilities(client: &Client) -> CheckResult<()> {
    let Payload::Hello {
        agent: Some(info), ..
    } = client.handshake()
    else {
        return Err("Maven acceptance agent metadata missing".into());
    };
    require(
        !info.supports("language_start")
            && JAVA_LANGUAGE_SESSION_CAPABILITIES
                .iter()
                .all(|name| info.supports(name))
            && info.supports("language_start_java_maven_begin")
            && info.supports("language_maven_model"),
        "normal Windows agent Maven capabilities incomplete",
    )
}

fn await_start(
    client: &mut Client,
    id: u64,
    java: &Path,
    observed: &mut Option<RootObservation>,
    deadline: Instant,
) -> CheckResult<Value> {
    while Instant::now() < deadline {
        let value = language_value(client, Operation::LanguageStartJavaPoll { startup_id: id })?;
        require(
            Instant::now() < deadline,
            "Maven startup poll exceeded its original budget",
        )?;
        require(value["startup_id"] == id, "Maven startup identity changed")?;
        if observed.is_none() {
            if let Some(pid) = value["process_id"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
            {
                let process = RootObservation::open_current(1, pid)?;
                verify_image(&process, java)?;
                *observed = Some(process);
            }
        }
        match value["state"].as_str() {
            Some("ready") => return Ok(value["language"].clone()),
            Some("starting") => {}
            _ => return Err("Maven startup failed or returned an invalid state".into()),
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("Maven startup exceeded its 75-second budget".into())
}

fn cancel_start(client: &mut Client, id: u64, deadline: Instant) -> CheckResult<()> {
    let mut value = language_value(
        client,
        Operation::LanguageStartJavaCancel { startup_id: id },
    )?;
    loop {
        require(
            value["startup_id"] == id,
            "Maven startup cancellation identity mismatch",
        )?;
        match value["state"].as_str() {
            Some("cancelled" | "failed") => {
                return require(
                    value["cleanup_verified"] == true,
                    "Maven cancellation cleanup not verified",
                )
            }
            Some("cancelling") => {}
            _ => return Err("Maven startup cancellation returned an invalid state".into()),
        }
        require(
            Instant::now() < deadline,
            "Maven startup cancellation exceeded cleanup budget",
        )?;
        thread::sleep(Duration::from_millis(50));
        value = language_value(client, Operation::LanguageStartJavaPoll { startup_id: id })?;
    }
}

fn verified_cache() -> CheckResult<(PathBuf, Inventory)> {
    let root = environment_path("CEDAR_MAVEN_CACHE_INPUT")?;
    require(
        root.is_dir(),
        "Maven cache input must be an existing directory",
    )?;
    let inventory = snapshot(&root)?;
    // The public provisioning manifest contains only pinned public artifacts.
    // The imported leaf never provisions these files or contacts a repository.
    let manifest: Value =
        serde_json::from_str(include_str!("../../../scripts/maven_cache_manifest.json"))
            .map_err(|_| "invalid public Maven cache manifest")?;
    let entries = manifest["entries"]
        .as_array()
        .ok_or("Maven manifest entries missing")?;
    require(
        entries.len() == CACHE_FILES
            && inventory.len() == CACHE_FILES
            && manifest["expected_files"] == CACHE_FILES as u64
            && manifest["expected_bytes"] == 4_065_288u64,
        "Maven cache does not match the exact public 83-file input contract",
    )?;
    let mut declared = Inventory::new();
    for entry in entries {
        let path = PathBuf::from(
            entry["path"]
                .as_str()
                .ok_or("cache manifest path missing")?,
        );
        require(
            !path.is_absolute()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "cache manifest path is not confined",
        )?;
        let seal = Seal {
            bytes: entry["bytes"]
                .as_u64()
                .ok_or("cache manifest size missing")?,
            sha256: entry["sha256"]
                .as_str()
                .ok_or("cache manifest hash missing")?
                .into(),
        };
        require(
            declared.insert(path, seal).is_none(),
            "duplicate cache manifest file",
        )?;
    }
    require(
        inventory == declared
            && inventory.values().map(|seal| seal.bytes).sum::<u64>() == 4_065_288,
        "Maven cache paths, sizes or hashes differ from the pinned manifest",
    )?;
    require(
        absent(&root.join(DEPENDENCY_JAR))? && absent(&root.join(DEPENDENCY_POM))?,
        "public cache input unexpectedly contains the generated dependency",
    )?;
    Ok((root, inventory))
}

fn copy_cache(input: &Path, destination: &Path, inventory: &Inventory) -> CheckResult<()> {
    use std::io::Write;

    for (relative, expected) in inventory {
        require(
            expected.bytes <= 8 * 1024 * 1024,
            "pinned cache input exceeds its bounded read limit",
        )?;
        let source = input.join(relative);
        let metadata = io(fs::symlink_metadata(&source))?;
        require(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.file_attributes() & 0x400 == 0
                && metadata.len() == expected.bytes,
            "pinned cache input changed before copying",
        )?;
        let mut contents = Vec::new();
        io(io(fs::File::open(source))?
            .take(expected.bytes + 1)
            .read_to_end(&mut contents))?;
        require(
            contents.len() as u64 == expected.bytes && hash(&contents) == expected.sha256,
            "cache source differs from its pinned input",
        )?;
        let target = destination.join(relative);
        io(fs::create_dir_all(
            target.parent().ok_or("cache destination parent missing")?,
        ))?;
        require(absent(&target)?, "cache destination was not fresh")?;
        // Create a fresh writable owned file without copying readonly attributes
        // or mutating any permissions. The sealed source remains untouched.
        let mut file = io(fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target))?;
        io(file.write_all(&contents))?;
    }
    Ok(())
}

struct CasePaths {
    root: PathBuf,
    repository: PathBuf,
    data: PathBuf,
}
fn setup_case(
    base: &Path,
    input: &Path,
    cache: &Inventory,
    kind: CaseKind,
) -> CheckResult<(CasePaths, Inventory)> {
    require(
        text(base)?.is_ascii(),
        "Maven control/data fixture parent must be ASCII",
    )?;
    let paths = CasePaths {
        root: base.join("workspace 雪"),
        repository: base.join("repository 雪"),
        data: base.join("data"),
    };
    io(fs::create_dir_all(paths.root.join("source-java/demo")))?;
    io(fs::create_dir(&paths.repository))?;
    io(fs::create_dir(&paths.data))?;
    io(fs::write(paths.root.join(SOURCE_FILE), FIXTURE_SOURCE))?;
    io(fs::write(paths.root.join("pom.xml"), FIXTURE_POM))?;
    copy_cache(input, &paths.repository, cache)?;
    if kind.present() {
        io(fs::create_dir_all(
            paths.repository.join(DEPENDENCY_DIRECTORY),
        ))?;
        io(fs::write(
            paths.repository.join(DEPENDENCY_JAR),
            sealed_jar()?,
        ))?;
        io(fs::write(
            paths.repository.join(DEPENDENCY_POM),
            ARTIFACT_POM,
        ))?;
    }
    for marker in [
        ".cedar-windows-language-validation",
        ".cedar-windows-java-validation",
        ".cedar-windows-java-gc-diagnostic",
        ".project",
        ".classpath",
        ".settings",
    ] {
        require(
            absent(&paths.root.join(marker))?,
            "Maven leaf fixture contains preexisting execution or project markers",
        )?;
    }
    let copied = snapshot(&paths.repository)?;
    require(
        copied.len() == CACHE_FILES + if kind.present() { 2 } else { 0 },
        "Maven case cache file count mismatch",
    )?;
    Ok((paths, copied))
}

fn source_unchanged(paths: &CasePaths) -> bool {
    fs::read(paths.root.join(SOURCE_FILE))
        .is_ok_and(|bytes| hash(&bytes) == hash(FIXTURE_SOURCE.as_bytes()))
}
fn fresh_control_directory(paths: &CasePaths) -> CheckResult<PathBuf> {
    let data_metadata = io(fs::symlink_metadata(&paths.data))?;
    require(
        data_metadata.is_dir()
            && !data_metadata.file_type().is_symlink()
            && data_metadata.file_attributes() & 0x400 == 0
            && text(&paths.data)?.is_ascii()
            && ordinary_path(&paths.data)? == paths.data,
        "Maven selected data directory identity changed",
    )?;
    let entries = io(fs::read_dir(&paths.data))?
        .take(2)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "control directory read failed")?;
    require(
        entries.len() == 1,
        "Maven controls must use exactly one fresh owned directory",
    )?;
    let entry = &entries[0];
    let control = entry.path();
    let metadata = io(fs::symlink_metadata(&control))?;
    require(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.file_attributes() & 0x400 == 0
            && entry.file_name().to_str().is_some_and(|name| {
                name.strip_prefix("cedar-maven-").is_some_and(|suffix| {
                    !suffix.is_empty()
                        && suffix.len() <= 64
                        && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
                })
            })
            && text(&control)?.is_ascii()
            && ordinary_path(&control)? == control,
        "Maven control must be the one fresh ordinary Cedar-owned child",
    )?;
    Ok(control)
}

fn verify_control(paths: &CasePaths) -> CheckResult<()> {
    let control = fresh_control_directory(paths)?;
    require(
        control.join("home").is_dir()
            && control.join("tmp").is_dir()
            && control.join("jdt-data/.metadata").is_dir()
            && control.join("user-settings.xml").is_file()
            && control.join("global-settings.xml").is_file(),
        "Maven data, controls or isolated home layout mismatch",
    )
}

fn owned_mirror_uri(paths: &CasePaths) -> CheckResult<String> {
    // Derive authority from the fixture's sole fresh control subtree, never
    // from a URL or filename supplied by a generated cache file.
    let mirror = fresh_control_directory(paths)?.join("empty-mirror");
    let metadata = io(fs::symlink_metadata(&mirror))?;
    require(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.file_attributes() & 0x400 == 0
            && ordinary_path(&mirror)? == mirror
            && io(fs::read_dir(&mirror))?.next().is_none(),
        "Maven owned file mirror must remain an empty ordinary directory",
    )?;
    directory_url(&mirror)
}

fn same_path(actual: &str, expected: &Path) -> bool {
    expected.to_str().is_some_and(|expected| {
        actual
            .replace('\\', "/")
            .eq_ignore_ascii_case(&expected.replace('\\', "/"))
    })
}
fn inspect_model(value: &Value, paths: &CasePaths, record: &mut CaseEvidence) -> CheckResult<bool> {
    require(
        value["profile"] == "maven_leaf"
            && value["pom_path"] == "pom.xml"
            && value["pom_sha256"] == hash(FIXTURE_POM.as_bytes())
            && value["restart_required"] == false,
        "Maven model profile or owned POM hash mismatch",
    )?;
    record.model_status = match value["status"].as_str() {
        Some("imported") => ModelStatus::Imported,
        Some("unresolved") => ModelStatus::Unresolved,
        Some("unavailable") => ModelStatus::Unavailable,
        _ => return Err("invalid Maven model status".into()),
    };
    record.maven_nature = value["maven_nature"] == true;
    record.custom_source = value["source_paths"]
        .as_array()
        .is_some_and(|paths| paths.iter().any(|path| path == "source-java"));
    record.compiler_17 = value["compiler"]["source"] == "17"
        && value["compiler"]["compliance"] == "17"
        && value["compiler"]["target"] == "17";
    if !(record.maven_nature && record.custom_source && record.compiler_17) {
        return Ok(false);
    }
    let entries = value["classpath"]
        .as_array()
        .ok_or("Maven classpath missing")?;
    require(entries.len() <= 256, "Maven model exceeded classpath bound")?;
    let mut expected_count = 0;
    let mut foreign_count = 0;
    for entry in entries {
        match entry["kind"].as_str() {
            Some("source") => {
                let source = entry["path"].as_str().ok_or("Maven source path missing")?;
                let relative = Path::new(source);
                require(
                    !relative.is_absolute()
                        && relative
                            .components()
                            .all(|part| matches!(part, Component::Normal(_))),
                    "Maven source path escaped the generated leaf",
                )?;
            }
            Some("library") => {
                if entry["path"]
                    .as_str()
                    .is_some_and(|path| same_path(path, &paths.repository.join(DEPENDENCY_JAR)))
                {
                    expected_count += 1;
                    require(
                        entry["resolved"] == record.case.present(),
                        "Maven dependency resolution disagrees with actual case cache",
                    )?;
                    require(
                        entry["origin"] == "model"
                            || (!record.case.present() && entry["origin"] == "declared"),
                        "Maven dependency reference has an unexpected origin",
                    )?;
                } else {
                    foreign_count += 1;
                }
            }
            Some("container") => {
                if !entry["path"]
                    .as_str()
                    .is_some_and(|path| path.starts_with("org.eclipse.jdt.launching.JRE_CONTAINER"))
                {
                    foreign_count += 1;
                }
            }
            _ => return Err("unexpected Maven classpath entry kind".into()),
        }
    }
    record.unexpected_dependency_references = foreign_count;
    // An exact kind-1 reference to the deliberately absent JAR is legitimate
    // negative evidence. It is not classified as a foreign dependency.
    require(
        foreign_count == 0 && expected_count <= 1,
        "Maven model included a foreign or extra dependency reference",
    )?;
    record.exact_dependency_reference = expected_count == 1;
    let expected_status = if record.case.present() {
        ModelStatus::Imported
    } else {
        ModelStatus::Unresolved
    };
    Ok(record.exact_dependency_reference
        && record.model_status == expected_status
        && value["unresolved_count"] == if record.case.present() { 0 } else { 1 })
}

#[derive(Default)]
struct DiagnosticState {
    offline_pom: bool,
    type_error: bool,
    corrected: bool,
}
fn exact_type_diagnostic(diagnostic: &Value) -> bool {
    diagnostic["severity"] == 1
        && diagnostic["code"] == "16777233"
        && diagnostic["message"] == "Type mismatch: cannot convert from int to String"
        && diagnostic["range"]
            == serde_json::json!({"start":{"line":5,"character":25},"end":{"line":5,"character":30}})
}
fn inspect_events(
    value: &Value,
    paths: &CasePaths,
    kind: CaseKind,
    diagnostics: &mut DiagnosticState,
    correction: bool,
) -> CheckResult<()> {
    require(
        value["truncated"] == false,
        "Maven event stream was truncated",
    )?;
    let events = value["events"].as_array().ok_or("Maven events missing")?;
    let source_uri =
        url::Url::from_file_path(paths.root.join(SOURCE_FILE)).map_err(|_| "source URI failed")?;
    let pom_uri =
        url::Url::from_file_path(paths.root.join("pom.xml")).map_err(|_| "POM URI failed")?;
    for event in events {
        match event["type"].as_str() {
            Some("notification" | "unsupported_server_request") => {}
            Some("diagnostics") => {
                let value = &event["value"];
                let uri = value["uri"]
                    .as_str()
                    .ok_or("Maven diagnostic URI missing")?;
                let items = value["diagnostics"]
                    .as_array()
                    .ok_or("Maven diagnostic list missing")?;
                if same_local_uri(uri, pom_uri.as_str()) {
                    for item in items {
                        require(
                            !kind.present()
                                && item["severity"] == 1
                                && item["code"] == "0"
                                && item["message"]
                                    == format!("Offline / Missing artifact {DEPENDENCY_GAV}"),
                            "Maven POM reported a foreign or unexpected diagnostic",
                        )?;
                        diagnostics.offline_pom = true;
                    }
                } else if same_local_uri(uri, source_uri.as_str()) {
                    if correction
                        && items.is_empty()
                        && value
                            .get("version")
                            .is_none_or(|version| version.is_null() || version == 2)
                    {
                        diagnostics.corrected = true;
                    }
                    for item in items {
                        if exact_type_diagnostic(item) {
                            diagnostics.type_error = true;
                        } else {
                            // The missing dependency can yield the two expected
                            // unresolved source references before didOpen too.
                            require(
                                !kind.present()
                                    && item["severity"] == 1
                                    && matches!(
                                        item["message"].as_str(),
                                        Some(
                                            "The import cedar cannot be resolved"
                                                | "Arithmetic cannot be resolved"
                                        )
                                    ),
                                "Maven source reported a foreign or unexpected diagnostic",
                            )?;
                        }
                    }
                } else {
                    require(
                        items.is_empty(),
                        "Maven diagnostics referenced a foreign document",
                    )?;
                }
            }
            _ => return Err("Maven event stream closed, lagged or was malformed".into()),
        }
    }
    Ok(())
}

fn await_model(
    client: &mut Client,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    diagnostics: &mut DiagnosticState,
) -> CheckResult<()> {
    let deadline = Instant::now() + MODEL_BUDGET;
    while record.model_queries < MAX_MODEL_QUERIES && Instant::now() < deadline {
        record.model_queries += 1;
        let model = language_value(client, Operation::LanguageMavenModel)?;
        let ready = inspect_model(&model, paths, record)?;
        let events = language_value(client, Operation::LanguageEvents)?;
        inspect_events(&events, paths, record.case, diagnostics, false)?;
        record.offline_pom_diagnostic = diagnostics.offline_pom;
        require(
            Instant::now() < deadline,
            "Maven model request exceeded its fixed budget",
        )?;
        if ready && (record.case.present() || diagnostics.offline_pom) {
            return Ok(());
        }
        if record.model_queries < MAX_MODEL_QUERIES
            && deadline.saturating_duration_since(Instant::now()) >= MODEL_INTERVAL
        {
            thread::sleep(MODEL_INTERVAL);
        } else {
            break;
        }
    }
    Err("Maven model did not establish its bounded present/missing witness".into())
}

fn semantic_queries(
    client: &mut Client,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    diagnostics: &mut DiagnosticState,
) -> CheckResult<()> {
    let deadline = Instant::now() + SEMANTIC_BUDGET;
    let opened = language_value(
        client,
        Operation::LanguageOpen {
            path: SOURCE_FILE.into(),
            language_id: "java".into(),
            version: 1,
            text: FIXTURE_SOURCE.into(),
        },
    )?;
    let uri = opened["opened"]
        .as_str()
        .ok_or("Maven source open URI missing")?;
    let expected = url::Url::from_file_path(paths.root.join(SOURCE_FILE))
        .map_err(|_| "Maven source URI failed")?;
    require(
        opened["version"] == 1 && same_local_uri(uri, expected.as_str()),
        "Maven didOpen identity mismatch",
    )?;
    let cursor = completion::byte_to_position(
        FIXTURE_SOURCE,
        FIXTURE_SOURCE
            .find("Arithmetic.answer")
            .ok_or("fixture call missing")?
            + "Arithmetic.an".len(),
    )?;
    let hover = language_value(
        client,
        Operation::LanguageQuery {
            path: SOURCE_FILE.into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Hover,
        },
    )?;
    let hover = crate::language_results::hover_text(&hover);
    require(
        hover.contains("int cedar.fixture.Arithmetic.answer()"),
        "Maven hover did not resolve the exact generated dependency method",
    )?;
    record.hover = true;
    require(
        Instant::now() < deadline,
        "Maven semantic budget expired after hover",
    )?;
    let completion = language_value(
        client,
        Operation::LanguageQuery {
            path: SOURCE_FILE.into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Completion,
        },
    )?;
    let completion = completion::parse_completion_result(&completion)?;
    require(
        completion.candidates.iter().any(|item| {
            item.label == "answer() : int"
                && item.item["detail"] == "Arithmetic.answer() : int"
                && item.item["textEdit"]["newText"] == "answer"
        }),
        "Maven completion did not resolve the exact generated dependency method",
    )?;
    record.completion = true;
    while Instant::now() < deadline && !diagnostics.type_error {
        let events = language_value(client, Operation::LanguageEvents)?;
        inspect_events(&events, paths, record.case, diagnostics, false)?;
        if !diagnostics.type_error {
            thread::sleep(MODEL_INTERVAL);
        }
    }
    require(
        diagnostics.type_error && Instant::now() < deadline,
        "Maven deliberate int-to-String diagnostic missing",
    )?;
    record.deliberate_type_diagnostic = true;
    let mut document = Document::new(1, SOURCE_FILE.into(), FIXTURE_SOURCE.into(), "r0".into());
    let corrected = FIXTURE_SOURCE.replace(
        "String invalid = known;",
        "String invalid = Integer.toString(known);",
    );
    editor_state::commit(
        &eframe::egui::Context::default(),
        &mut document,
        corrected.clone(),
        0,
    );
    require(
        document.dirty()
            && document.saved_text == FIXTURE_SOURCE
            && document.text == corrected
            && document.edit_version == 1
            && source_unchanged(paths),
        "Maven frontend transaction lost its dirty baseline",
    )?;
    let changed = language_value(
        client,
        Operation::LanguageChange {
            path: document.path.clone(),
            version: 2,
            text: document.text.clone(),
        },
    )?;
    require(
        changed["version"] == 2
            && changed["changed"]
                .as_str()
                .is_some_and(|actual| same_local_uri(actual, uri)),
        "Maven dirty didChange acknowledgment mismatch",
    )?;
    record.dirty_change_acknowledged = true;
    while Instant::now() < deadline && !diagnostics.corrected {
        let events = language_value(client, Operation::LanguageEvents)?;
        inspect_events(&events, paths, record.case, diagnostics, true)?;
        if !diagnostics.corrected {
            thread::sleep(MODEL_INTERVAL);
        }
    }
    require(
        diagnostics.corrected
            && Instant::now() < deadline
            && document.dirty()
            && source_unchanged(paths),
        "Maven unsaved correction did not clear diagnostics without saving",
    )?;
    record.no_autosave = true;
    language_value(
        client,
        Operation::LanguageClose {
            path: SOURCE_FILE.into(),
        },
    )?;
    require(
        Instant::now() < deadline,
        "Maven semantics exceeded the fixed 20-second budget",
    )
}

fn check_repository(
    paths: &CasePaths,
    before: &Inventory,
    record: &mut CaseEvidence,
) -> CheckResult<()> {
    record.dependency_jar_present_after = !absent(&paths.repository.join(DEPENDENCY_JAR))?;
    record.dependency_pom_present_after = !absent(&paths.repository.join(DEPENDENCY_POM))?;
    // Check every immutable input independently before inspecting any new
    // metadata. Oversized or invalid new files cannot suppress these checks.
    record.repository_inputs_unchanged = before.iter().all(|(relative, seal)| {
        let path = paths.repository.join(relative);
        fs::symlink_metadata(&path).is_ok_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.file_attributes() & 0x400 == 0
                && metadata.len() == seal.bytes
                && fs::read(&path).is_ok_and(|bytes| hash(&bytes) == seal.sha256)
        })
    });
    let mirror_uri = owned_mirror_uri(paths)?;
    let after = snapshot(&paths.repository)?;
    for (path, seal) in &after {
        if before.contains_key(path) {
            continue;
        }
        let allowed_parent = path.parent().is_some_and(|parent| {
            parent == Path::new(DEPENDENCY_DIRECTORY)
                || before.keys().any(|known| known.parent() == Some(parent))
        });
        let allowed_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                matches!(
                    name,
                    "_remote.repositories"
                        | "resolver-status.properties"
                        | "m2e-lastUpdated.properties"
                ) || name.ends_with(".lastUpdated")
            });
        if allowed_parent && allowed_name && seal.bytes <= 32 * 1024 {
            record.generated_metadata_files += 1;
        } else {
            let lifecycle_bit = checked(|| {
                use crate::java_maven_metadata_tests::{
                    lifecycle_metadata_bit, MAX_METADATA_BYTES,
                };
                require(
                    seal.bytes <= MAX_METADATA_BYTES as u64,
                    "generated lifecycle metadata exceeds its existing file bound",
                )?;
                let full = paths.repository.join(path);
                let metadata = io(fs::symlink_metadata(&full))?;
                require(
                    metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && metadata.file_attributes() & 0x400 == 0
                        && metadata.len() == seal.bytes,
                    "generated lifecycle metadata is not the inspected ordinary file",
                )?;
                let mut contents = Vec::new();
                io(io(fs::File::open(full))?
                    .take(MAX_METADATA_BYTES as u64 + 1)
                    .read_to_end(&mut contents))?;
                require(
                    contents.len() as u64 == seal.bytes && hash(&contents) == seal.sha256,
                    "generated lifecycle metadata changed during inspection",
                )?;
                lifecycle_metadata_bit(path, &contents, &mirror_uri)
                    .ok_or_else(|| "generated file is not exact owned lifecycle metadata".into())
            });
            match lifecycle_bit {
                Ok(bit) if record.lifecycle_metadata_mask & bit == 0 => {
                    record.lifecycle_metadata_mask |= bit;
                    record.lifecycle_metadata_files += 1;
                    record.generated_metadata_files += 1;
                }
                _ => record.foreign_repository_files += 1,
            }
        }
    }
    require(
        record.repository_inputs_unchanged
            && record.foreign_repository_files == 0
            && record.generated_metadata_files <= 128
            && record.lifecycle_metadata_files <= 6
            && record.lifecycle_metadata_mask <= 63
            && u32::from(record.lifecycle_metadata_files)
                == record.lifecycle_metadata_mask.count_ones()
            && record.dependency_jar_present_after == record.case.present()
            && record.dependency_pom_present_after == record.case.present(),
        "Maven changed sealed repository inputs or materialized an unapproved artifact",
    )
}

fn generated_metadata(root: &Path, project: bool) -> CheckResult<(u32, u64)> {
    let mut directories = vec![root.to_path_buf()];
    let mut entries = 0u32;
    let mut files = 0u32;
    let mut bytes = 0u64;
    while let Some(directory) = directories.pop() {
        for entry in io(fs::read_dir(directory))? {
            entries += 1;
            require(
                entries <= if project { 512 } else { 4096 },
                "generated Maven metadata exceeded its entry bound",
            )?;
            let path = io(entry)?.path();
            let metadata = io(fs::symlink_metadata(&path))?;
            require(
                !metadata.file_type().is_symlink() && metadata.file_attributes() & 0x400 == 0,
                "generated Maven metadata contains a link or reparse point",
            )?;
            if metadata.is_dir() {
                directories.push(path);
                continue;
            }
            require(
                metadata.is_file(),
                "generated Maven metadata is not an ordinary file",
            )?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "generated metadata escaped its root")?;
            if project {
                if relative == Path::new(SOURCE_FILE) || relative == Path::new("pom.xml") {
                    continue;
                }
                require(
                    relative == Path::new(".project")
                        || relative == Path::new(".classpath")
                        || relative.starts_with(".settings"),
                    "Maven leaf contains an unexpected generated file",
                )?;
            }
            files += 1;
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or("generated metadata byte counter overflow")?;
            require(
                files <= if project { 256 } else { 4096 }
                    && bytes
                        <= if project {
                            16 * 1024 * 1024
                        } else {
                            128 * 1024 * 1024
                        },
                "generated Maven metadata exceeded its aggregate file or byte bound",
            )?;
            // Deliberately do not read or hash generated JDT indexes. A normal
            // JDK index can exceed 40 MiB; only original inputs use hash seals.
        }
    }
    Ok((files, bytes))
}

fn case_passed(record: &CaseEvidence) -> bool {
    record.java_capabilities
        && record.generic_start_rejected
        && record.untrusted_start_rejected
        && record.model_without_session_rejected
        && record.async_start_begin_acknowledged
        && record.async_start_read_while_starting
        && record.async_start_ready
        && record.root_identity_verified
        && record.root_observed_live
        && record.maven_nature
        && record.custom_source
        && record.compiler_17
        && record.exact_dependency_reference
        && record.unexpected_dependency_references == 0
        && record.model_queries > 0
        && record.model_queries <= MAX_MODEL_QUERIES
        && record.stale_startup_rejected
        && record.changed_pom_restart_required
        && record.source_unchanged
        && record.pom_expected
        && record.repository_inputs_unchanged
        && record.foreign_repository_files == 0
        && record.generated_metadata_files <= 128
        && record.lifecycle_metadata_files <= 6
        && record.lifecycle_metadata_mask <= 63
        && u32::from(record.lifecycle_metadata_files) == record.lifecycle_metadata_mask.count_ones()
        && record.generated_data_files > 0
        && record.generated_data_files <= 4096
        && record.generated_data_bytes > 0
        && record.generated_data_bytes <= 128 * 1024 * 1024
        && record.generated_project_files <= 256
        && record.generated_project_bytes <= 16 * 1024 * 1024
        && record.dependency_jar_present_before == record.case.present()
        && record.dependency_jar_present_after == record.case.present()
        && record.dependency_pom_present_before == record.case.present()
        && record.dependency_pom_present_after == record.case.present()
        && record.stop_outcome_verified
        && record.cleanup_joined
        && record.root_handle_signaled
        && record.root_exit_code.is_some()
        && record.model_after_stop_rejected
        && record.client_reaped
        && record.synthetic_root_removed
        && !record.primary_failed
        && !record.cleanup_failed
        && if record.case.present() {
            record.model_status == ModelStatus::Imported
                && record.hover
                && record.completion
                && record.deliberate_type_diagnostic
                && record.dirty_change_acknowledged
                && record.no_autosave
        } else {
            record.model_status == ModelStatus::Unresolved && record.offline_pom_diagnostic
        }
}

fn run_case(
    kind: CaseKind,
    binary: &Path,
    java: &Path,
    distribution: &Path,
    input: &Path,
    cache: &Inventory,
    pair_deadline: Instant,
) -> CaseEvidence {
    let mut record = CaseEvidence {
        case: kind,
        client_reaped: true,
        ..CaseEvidence::default()
    };
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut paths: Option<CasePaths> = None;
    let mut repository_before = Inventory::new();
    let mut client: Option<Client> = None;
    let mut observed: Option<RootObservation> = None;
    let mut startup_id = None;
    let mut ready = false;
    let mut pom_changed = false;
    let primary = checked(|| {
        admit_work(pair_deadline, case_budget(kind), Duration::ZERO)?;
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar-maven-")
            .tempdir())?);
        let base = ordinary_path(fixture.as_ref().ok_or("Maven fixture missing")?.path())?;
        let (prepared, inventory) = setup_case(&base, input, cache, kind)?;
        paths = Some(prepared);
        repository_before = inventory;
        let paths = paths.as_ref().ok_or("Maven paths missing")?;
        record.dependency_jar_present_before = !absent(&paths.repository.join(DEPENDENCY_JAR))?;
        record.dependency_pom_present_before = !absent(&paths.repository.join(DEPENDENCY_POM))?;
        require(
            record.dependency_jar_present_before == kind.present()
                && record.dependency_pom_present_before == kind.present(),
            "Maven dependency preflight presence mismatch",
        )?;
        let start = || -> CheckResult<Operation> {
            Ok(Operation::LanguageStartJavaMavenBegin {
                java_executable: text(java)?,
                distribution: text(distribution)?,
                data_directory: text(&paths.data)?,
                local_repository: text(&paths.repository)?,
            })
        };
        record.failure_stage = Stage::Trust;
        admit_work(pair_deadline, case_budget(kind), Duration::ZERO)?;
        record.client_reaped = false;
        client = Some(Client::spawn_agent(binary, &paths.root, false)?);
        capabilities(client.as_ref().ok_or("Maven untrusted Client missing")?)?;
        rejected(
            client.as_mut().ok_or("Maven untrusted Client missing")?,
            start()?,
            "run_disabled",
        )?;
        require(
            io(fs::read_dir(&paths.data))?.next().is_none(),
            "untrusted Maven startup created controls",
        )?;
        record.untrusted_start_rejected = true;
        rejected(
            client.as_mut().ok_or("Maven untrusted Client missing")?,
            Operation::LanguageStart {
                program: "must-not-run".into(),
                args: vec![],
            },
            "unsupported_operation",
        )?;
        record.generic_start_rejected = true;
        admit_work(pair_deadline, CLIENT_REAP_BUDGET, CLEANUP_BUDGET)?;
        let reap_budget =
            CLIENT_REAP_BUDGET.min(pair_deadline.saturating_duration_since(Instant::now()));
        client
            .take()
            .ok_or("Maven untrusted Client missing")?
            .close_and_wait(reap_budget)?;
        record.client_reaped = true;
        // This is explicit test trust on a newly generated workspace, never a
        // GUI trust action and never inherited from a user workspace.
        admit_work(pair_deadline, case_budget(kind), Duration::ZERO)?;
        record.client_reaped = false;
        client = Some(Client::spawn_agent(binary, &paths.root, true)?);
        let client = client.as_mut().ok_or("Maven trusted Client missing")?;
        capabilities(client)?;
        record.java_capabilities = true;
        rejected(
            client,
            Operation::LanguageMavenModel,
            "language_not_running",
        )?;
        record.model_without_session_rejected = true;
        record.failure_stage = Stage::Startup;
        admit_work(
            pair_deadline,
            START_TIMEOUT,
            MODEL_BUDGET + semantic_budget(kind) + CLEANUP_BUDGET,
        )?;
        let startup_deadline = Instant::now() + START_TIMEOUT;
        let started = language_value(client, start()?)?;
        let id = started["startup_id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or("Maven Begin omitted startup identity")?;
        startup_id = Some(id);
        require(
            started["state"] == "starting",
            "Maven Begin did not acknowledge asynchronous Starting",
        )?;
        record.async_start_begin_acknowledged = true;
        let read = client.request(Operation::Read {
            path: SOURCE_FILE.into(),
        })?;
        require(
            matches!(read, Payload::File { path, text, .. } if path == SOURCE_FILE && text == FIXTURE_SOURCE),
            "Maven normal read while startup pending failed",
        )?;
        record.async_start_read_while_starting = true;
        let initialized = await_start(client, id, java, &mut observed, startup_deadline)?;
        ready = initialized["started"] == true;
        require(
            ready
                && initialized["initialize"]["cedar_java_profile"] == "maven_leaf"
                && initialized["initialize"]["cedar_java_maven_model"] == true
                && initialized["initialize"]["cedar_java_maven_pom_sha256"]
                    == hash(FIXTURE_POM.as_bytes()),
            "Maven Ready lacks exact supported profile and POM identity",
        )?;
        record.async_start_ready = true;
        let pid = initialized["process_id"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .ok_or("Maven Ready root PID missing")?;
        if observed.is_none() {
            observed = Some(RootObservation::open_current(1, pid)?);
        }
        let process = observed.as_ref().ok_or("Maven root observation missing")?;
        require(
            process.pid == pid && process.live()?,
            "Maven Ready changed the live owned Java root",
        )?;
        record.root_observed_live = true;
        verify_image(process, java)?;
        record.root_identity_verified = true;
        verify_control(paths)?;
        admit_work(
            pair_deadline,
            MODEL_BUDGET + semantic_budget(kind),
            CLEANUP_BUDGET,
        )?;
        rejected(
            client,
            Operation::LanguageStartJavaPoll {
                startup_id: id.checked_add(1).ok_or("Maven startup ID overflow")?,
            },
            "unknown_language_startup",
        )?;
        record.stale_startup_rejected = true;
        let mut diagnostics = DiagnosticState::default();
        record.failure_stage = Stage::Model;
        admit_work(
            pair_deadline,
            MODEL_BUDGET,
            semantic_budget(kind) + CLEANUP_BUDGET,
        )?;
        await_model(client, paths, &mut record, &mut diagnostics)?;
        if kind.present() {
            record.failure_stage = Stage::Semantics;
            admit_work(pair_deadline, SEMANTIC_BUDGET, CLEANUP_BUDGET)?;
            semantic_queries(client, paths, &mut record, &mut diagnostics)?;
        }
        require(
            process.live()? && source_unchanged(paths),
            "Maven root died or source was saved before explicit Stop",
        )?;
        record.failure_stage = Stage::PomChange;
        admit_work(pair_deadline, Duration::ZERO, CLEANUP_BUDGET)?;
        // A deliberate change to this owned input must invalidate the old
        // model. No implicit reimport or new session is permitted here.
        io(fs::write(
            paths.root.join("pom.xml"),
            format!("{FIXTURE_POM}\n"),
        ))?;
        pom_changed = true;
        rejected(
            client,
            Operation::LanguageMavenModel,
            "language_maven_restart_required",
        )?;
        record.changed_pom_restart_required = true;
        admit_work(pair_deadline, Duration::ZERO, CLEANUP_BUDGET)?;
        Ok(())
    });
    record.primary_failed = primary.is_err();
    let failure_stage = record.failure_stage;
    let cleanup_started = Instant::now();
    let cleanup_deadline = bounded_cleanup_deadline(cleanup_started, pair_deadline);
    let cleanup_admitted = budget_admits(
        cleanup_started,
        pair_deadline,
        CLEANUP_BUDGET,
        Duration::ZERO,
    );
    let cleanup = checked(|| {
        record.failure_stage = Stage::Stop;
        require(
            cleanup_admitted,
            "Maven cleanup did not retain its required observation reserve",
        )?;
        if ready {
            let client = client.as_mut().ok_or("Maven Client missing during Stop")?;
            let stopped = language_value(client, Operation::LanguageStop)?;
            require(
                Instant::now() < cleanup_deadline,
                "Maven Stop returned after the observation deadline",
            )?;
            let outcome = JavaStopOutcome::parse(&stopped)?;
            record.stop_status = Some(outcome.status);
            record.stop_reason = Some(outcome.reason);
            record.cleanup_joined = outcome.cleanup_joined;
            record.shutdown_response_received = outcome.shutdown_response_received;
            record.exit_frame_completed = outcome.exit_frame_completed;
            record.failure_stage = Stage::RootExit;
            let actual = observed
                .as_ref()
                .ok_or("Maven root handle missing")?
                .exit_code_with_timeout(0)?;
            record.root_handle_signaled = true;
            record.root_exit_code = Some(actual);
            require(
                actual == outcome.root_exit_code
                    && match outcome.status {
                        StopStatus::Graceful => true,
                        StopStatus::Forced => matches!(
                            outcome.reason,
                            StopReason::GraceExpired | StopReason::Aborted
                        ),
                        StopStatus::Error => false,
                    },
                "Maven typed Stop disagrees with retained root exit or reports errors",
            )?;
            record.stop_outcome_verified = true;
            admit_work(cleanup_deadline, Duration::ZERO, Duration::ZERO)?;
            // This normal no-session request can retain the Client's 30-second
            // RPC timeout. Admission is not a promise that it returns within
            // the remaining allowance: a late response must fail this case.
            rejected(
                client,
                Operation::LanguageMavenModel,
                "language_not_running",
            )?;
            require(
                Instant::now() < cleanup_deadline,
                "post-Stop model verification exceeded the remaining cleanup budget",
            )?;
            record.model_after_stop_rejected = true;
        } else if let (Some(id), Some(client)) = (startup_id, client.as_mut()) {
            cancel_start(client, id, cleanup_deadline)?;
        }
        require(
            Instant::now() < cleanup_deadline,
            "Maven Stop exceeded the fixed cleanup budget",
        )
    });
    record.cleanup_failed = cleanup.is_err();
    let cleanup_stage = record.failure_stage;
    // Closing the normal Client owns the agent reaper and its process/job
    // cleanup. Never kill a process discovered by PID or widen ownership.
    if let Some(client) = client.take() {
        let remaining = cleanup_deadline.saturating_duration_since(Instant::now());
        record.client_reaped = checked(|| client.close_and_wait(remaining)).is_ok();
        if !record.client_reaped {
            record.cleanup_failed = true;
            record.failure_stage = Stage::ClientExit;
        }
    }
    if let Some(process) = observed.as_ref() {
        match process.exit_code_with_timeout(0) {
            Ok(code) => {
                record.root_handle_signaled = true;
                record.root_exit_code = Some(code);
            }
            Err(_) => {
                record.cleanup_failed = true;
                record.failure_stage = Stage::RootExit;
            }
        }
    }
    if let Some(paths) = paths.as_ref() {
        record.source_unchanged = source_unchanged(paths);
        let expected_pom = if pom_changed {
            format!("{FIXTURE_POM}\n")
        } else {
            FIXTURE_POM.into()
        };
        record.pom_expected = fs::read(paths.root.join("pom.xml"))
            .is_ok_and(|bytes| hash(&bytes) == hash(expected_pom.as_bytes()));
        if checked(|| check_repository(paths, &repository_before, &mut record)).is_err()
            || !record.source_unchanged
            || !record.pom_expected
        {
            record.cleanup_failed = true;
            record.failure_stage = Stage::FixtureCleanup;
        }
        let metadata = checked(|| {
            let (files, bytes) = generated_metadata(&paths.data, false)?;
            record.generated_data_files = files;
            record.generated_data_bytes = bytes;
            let (files, bytes) = generated_metadata(&paths.root, true)?;
            record.generated_project_files = files as u16;
            record.generated_project_bytes = bytes;
            Ok(())
        });
        if metadata.is_err() {
            record.cleanup_failed = true;
            record.failure_stage = Stage::FixtureCleanup;
        }
    }
    let root_stopped = observed.is_none() || record.root_handle_signaled;
    if let Some(fixture) = fixture.take() {
        if record.client_reaped && root_stopped {
            record.synthetic_root_removed = fixture.close().is_ok();
            if !record.synthetic_root_removed {
                record.cleanup_failed = true;
                record.failure_stage = Stage::FixtureCleanup;
            }
        } else {
            // Retain only this generated fixture if ownership was not proved
            // closed. Deleting a live server's input is not cleanup evidence.
            let _ = fixture.keep();
        }
    }
    if Instant::now() >= cleanup_deadline || Instant::now() >= pair_deadline {
        record.cleanup_failed = true;
    }
    record.success = case_passed(&record);
    record.failure_stage = if record.success {
        Stage::None
    } else if record.primary_failed {
        failure_stage
    } else if cleanup.is_err() {
        cleanup_stage
    } else {
        record.failure_stage
    };
    record
}

#[test]
#[ignore = "requires native Windows, exact normal CEDAR_AGENT_BIN, CEDAR_JAVA, Unicode CEDAR_JDTLS_HOME, and pinned CEDAR_MAVEN_CACHE_INPUT; run serially"]
fn real_windows_normal_agent_java_maven_acceptance() -> CheckResult<()> {
    println!();
    let _watchdog = Watchdog::start_with_timeout(PAIR_BUDGET);
    let started = Instant::now();
    let pair_deadline = started + PAIR_BUDGET;
    let mut record = MavenEvidence {
        schema_version: 1,
        kind: "windows_java_maven",
        route: "normal_agent_normal_client",
        pair_count: 1,
        cache_input_files: 0,
        cache_input_unchanged: false,
        fixture_inputs_verified: false,
        source_sha256: SOURCE_SHA256,
        pom_sha256: POM_SHA256,
        dependency_jar_sha256: JAR_SHA256,
        present: CaseEvidence::default(),
        missing: CaseEvidence {
            case: CaseKind::Missing,
            ..CaseEvidence::default()
        },
        elapsed_ms: 0,
        elapsed_saturated: false,
        primary_failed: false,
        cleanup_failed: false,
        success: false,
    };
    let result = checked(|| {
        for name in [
            "CLIENT_PORT",
            "CLIENT_HOST",
            "socket.stream.debug",
            "JDK_JAVA_OPTIONS",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "MAVEN_OPTS",
            "MAVEN_ARGS",
            "MAVEN_CONFIG",
            "M2_HOME",
            "MAVEN_HOME",
        ] {
            require(
                std::env::var_os(name).is_none(),
                "unexpected Java or Maven launcher environment injection",
            )?;
        }
        let binary = environment_path("CEDAR_AGENT_BIN")?;
        let java = environment_path("CEDAR_JAVA")?;
        let distribution = environment_path("CEDAR_JDTLS_HOME")?;
        require(binary.is_file() && binary.file_name().is_some_and(|name| name == "cedar-agent.exe")
            && java.is_file() && java.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("java.exe"))
            && text(&java)?.is_ascii() && distribution.is_dir() && !text(&distribution)?.is_ascii(),
            "Maven acceptance requires exact normal agent, ASCII java.exe and Unicode JDT distribution")?;
        let (input, inventory) = verified_cache()?;
        record.cache_input_files = inventory.len() as u16;
        sealed_jar()?;
        require(
            hash(FIXTURE_SOURCE.as_bytes()) == SOURCE_SHA256
                && hash(FIXTURE_POM.as_bytes()) == POM_SHA256,
            "Maven fixture source/POM differ from their fixed receipts",
        )?;
        record.fixture_inputs_verified = true;
        record.present = run_case(
            CaseKind::Present,
            &binary,
            &java,
            &distribution,
            &input,
            &inventory,
            pair_deadline,
        );
        if prior_cleanup_verified(&record.present)
            && budget_admits(
                Instant::now(),
                pair_deadline,
                case_budget(CaseKind::Missing),
                Duration::ZERO,
            )
        {
            record.missing = run_case(
                CaseKind::Missing,
                &binary,
                &java,
                &distribution,
                &input,
                &inventory,
                pair_deadline,
            );
        } else {
            // Both cases remain mandatory. A skipped second launch is a failed
            // pair, never permission to retry or extend the outer watchdog.
            record.missing.primary_failed = true;
            record.missing.failure_stage = Stage::Setup;
        }
        record.cache_input_unchanged = snapshot(&input).is_ok_and(|after| after == inventory);
        Ok(())
    });
    record.primary_failed =
        result.is_err() || record.present.primary_failed || record.missing.primary_failed;
    record.cleanup_failed = record.present.cleanup_failed
        || record.missing.cleanup_failed
        || !record.cache_input_unchanged;
    let elapsed = started.elapsed().as_millis();
    record.elapsed_ms = elapsed.min(360_000) as u32;
    record.elapsed_saturated = elapsed > 360_000;
    record.success = !record.primary_failed
        && !record.cleanup_failed
        && !record.elapsed_saturated
        && record.fixture_inputs_verified
        && record.cache_input_files == 83
        && record.present.success
        && record.missing.success
        && Instant::now() < pair_deadline;
    println!(
        "{}",
        serde_json::to_string(&record).expect("typed Maven acceptance receipt")
    );
    require(
        record.success,
        "native normal-agent Maven acceptance failed; inspect typed per-case receipt",
    )
}

#[test]
fn maven_pair_budget_reserves_cleanup_and_rejects_late_or_unverified_work() {
    let now = Instant::now();
    let pair_deadline = now + PAIR_BUDGET;
    assert_eq!(CLEANUP_BUDGET, Duration::from_secs(90));
    assert_eq!(PAIR_BUDGET, Duration::from_secs(360));
    assert_eq!(case_budget(CaseKind::Present), Duration::from_secs(245));
    assert_eq!(case_budget(CaseKind::Missing), Duration::from_secs(225));
    assert!(budget_admits(
        now,
        pair_deadline,
        case_budget(CaseKind::Present),
        Duration::ZERO
    ));
    assert!(!budget_admits(
        now + Duration::from_secs(136),
        pair_deadline,
        case_budget(CaseKind::Missing),
        Duration::ZERO
    ));
    assert!(!budget_admits(
        pair_deadline,
        pair_deadline,
        Duration::ZERO,
        Duration::ZERO
    ));
    let clipped = bounded_cleanup_deadline(now + Duration::from_secs(300), pair_deadline);
    assert_eq!(clipped, pair_deadline);
    assert!(!budget_admits(
        now + Duration::from_secs(300),
        pair_deadline,
        CLEANUP_BUDGET,
        Duration::ZERO
    ));
    assert_eq!(
        bounded_cleanup_deadline(now, pair_deadline),
        now + CLEANUP_BUDGET
    );
    let mut prior = CaseEvidence {
        stop_outcome_verified: true,
        cleanup_joined: true,
        root_handle_signaled: true,
        root_exit_code: Some(0),
        client_reaped: true,
        synthetic_root_removed: true,
        ..CaseEvidence::default()
    };
    assert!(prior_cleanup_verified(&prior));
    prior.cleanup_failed = true;
    assert!(!prior_cleanup_verified(&prior));
    prior.cleanup_failed = false;
    prior.client_reaped = false;
    assert!(!prior_cleanup_verified(&prior));
}

#[test]
fn maven_missing_exact_model_reference_is_expected_negative_evidence() -> CheckResult<()> {
    let paths = CasePaths {
        root: PathBuf::from(r"C:\owned\workspace 雪"),
        repository: PathBuf::from(r"C:\owned\repository 雪"),
        data: PathBuf::from(r"C:\owned\data"),
    };
    let expected = serde_json::json!({
        "profile":"maven_leaf", "status":"unresolved", "pom_path":"pom.xml",
        "pom_sha256":POM_SHA256, "restart_required":false, "maven_nature":true,
        "compiler":{"source":"17","compliance":"17","target":"17"},
        "source_paths":["source-java"], "unresolved_count":1,
        "classpath":[{"kind":"source","path":"source-java","resolved":true,"origin":"model"},
            {"kind":"library","path":text(&paths.repository.join(DEPENDENCY_JAR))?,"resolved":false,"origin":"model"}]
    });
    let mut record = CaseEvidence {
        case: CaseKind::Missing,
        ..CaseEvidence::default()
    };
    assert!(inspect_model(&expected, &paths, &mut record)?);
    assert!(record.exact_dependency_reference);
    assert_eq!(record.unexpected_dependency_references, 0);
    let mut extra = expected.clone();
    extra["classpath"].as_array_mut().unwrap().push(serde_json::json!({
        "kind":"library","path":text(&paths.repository.join("foreign/extra.jar"))?,"resolved":false,"origin":"model"
    }));
    assert!(inspect_model(&extra, &paths, &mut record).is_err());
    assert_eq!(record.unexpected_dependency_references, 1);
    let mut duplicate = expected.clone();
    duplicate["classpath"]
        .as_array_mut()
        .unwrap()
        .push(expected["classpath"][1].clone());
    assert!(inspect_model(&duplicate, &paths, &mut record).is_err());
    Ok(())
}

#[test]
fn maven_offline_error_requires_exact_owned_pom_and_full_coordinate() -> CheckResult<()> {
    let paths = CasePaths {
        root: PathBuf::from(r"C:\owned\workspace 雪"),
        repository: PathBuf::from(r"C:\owned\repository 雪"),
        data: PathBuf::from(r"C:\owned\data"),
    };
    let uri =
        url::Url::from_file_path(paths.root.join("pom.xml")).map_err(|_| "test POM URI failed")?;
    let expected = serde_json::json!({"truncated":false,"events":[{"type":"diagnostics","value":{
        "uri":uri.as_str(),"diagnostics":[{"severity":1,"code":"0","message":format!("Offline / Missing artifact {DEPENDENCY_GAV}")}]}}]});
    let mut state = DiagnosticState::default();
    inspect_events(&expected, &paths, CaseKind::Missing, &mut state, false)?;
    assert!(state.offline_pom);
    for message in [
        "Missing artifact dev.cedar.fixture:arithmetic:jar:1.0.0",
        "Offline / Missing artifact dev.cedar.fixture:arithmetic:jar:2.0.0",
    ] {
        let mut wrong = expected.clone();
        wrong["events"][0]["value"]["diagnostics"][0]["message"] = message.into();
        assert!(inspect_events(
            &wrong,
            &paths,
            CaseKind::Missing,
            &mut DiagnosticState::default(),
            false
        )
        .is_err());
    }
    let mut foreign = expected.clone();
    foreign["events"][0]["value"]["uri"] = "file:///C:/foreign/pom.xml".into();
    assert!(inspect_events(
        &foreign,
        &paths,
        CaseKind::Missing,
        &mut DiagnosticState::default(),
        false
    )
    .is_err());
    assert!(inspect_events(
        &expected,
        &paths,
        CaseKind::Present,
        &mut DiagnosticState::default(),
        false
    )
    .is_err());
    Ok(())
}
