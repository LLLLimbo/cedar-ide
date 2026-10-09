//! Deliberately small, offline leaf-Maven launch profile.
//!
//! These checks constrain accepted configuration; they are not a filesystem or
//! network sandbox. The ordinary JDT process still has the account's permissions.
use crate::{error, io_error, java_launch};
use cedar_protocol::RemoteError;
use quick_xml::{events::Event, Reader};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const MAX_POM_BYTES: usize = 128 * 1024;
const MAX_XML_NODES: usize = 2048;
const MAX_XML_EVENTS: usize = 8192;
const MAX_XML_DEPTH: usize = 16;
const MAX_DEPENDENCIES: usize = 256;

#[derive(Debug)]
pub(super) struct MavenSession {
    pub root: PathBuf,
    pub local_repository: PathBuf,
    pub pom_sha256: String,
    pub pom_uri: String,
    pub declared_dependencies: Vec<PathBuf>,
    pub source_paths: Vec<String>,
}

fn invalid(message: &'static str) -> RemoteError {
    error("invalid_java_maven", message)
}

pub(super) fn production(
    root: &Path,
    java_executable: &str,
    distribution: &str,
    data_directory: &str,
    local_repository: &str,
) -> Result<java_launch::JavaLaunch, RemoteError> {
    check_environment()?;
    for text in [
        java_executable,
        distribution,
        data_directory,
        local_repository,
    ] {
        if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control) {
            return Err(invalid(
                "Maven host paths must contain 1..4096 bytes without controls",
            ));
        }
    }
    if !data_directory.is_ascii() {
        return Err(invalid(
            "Maven control data requires an ASCII path for the JDK launcher",
        ));
    }
    // Workspace stores a canonical path, including Windows' verbatim-disk
    // prefix. Convert that internal spelling before applying the public-path
    // rules; caller-supplied launch paths still require ordinary spelling.
    let root = java_launch::ordinary_local_path(root)?;
    let root = existing_path(&root, true)?;
    existing_path(Path::new(java_executable), false)?;
    let distribution_path = existing_path(Path::new(distribution), true)?;
    existing_path(&distribution_path.join("config_win"), true)?;
    existing_path(&distribution_path.join("plugins"), true)?;
    let data = outside_project(&root, Path::new(data_directory))?;
    let repository = outside_project(&root, Path::new(local_repository))?;
    reject_project_configuration(&root)?;
    let bytes = read_pom(&root)?;
    let model = parse_pom(&bytes)?;
    for relative in model.source_paths.iter().chain(model.output_paths.iter()) {
        confined_path(&root, relative, true)?;
    }
    let declared_dependencies = model
        .dependencies
        .iter()
        .map(|dependency| confined_path(&repository, &dependency.repository_path(), false))
        .collect::<Result<Vec<_>, _>>()?;
    let pom_uri = url::Url::from_file_path(root.join("pom.xml"))
        .map(String::from)
        .map_err(|_| invalid("Cannot encode the selected root POM as a file URI"))?;
    let mut launch = java_launch::production(&root, java_executable, distribution, data_directory)?;
    // The control directory is newly allocated, never a caller-selected config
    // directory. Keep its TempDir guard until every fallible preparation succeeds.
    let controls = tempfile::Builder::new()
        .prefix("cedar-maven-")
        .tempdir_in(&data)
        .map_err(io_error)?;
    let locations = create_controls(controls.path(), &repository)?;
    let home = ascii_path(&locations.home)?;
    let temporary = ascii_path(&locations.temporary)?;
    let data_uri = java_launch::directory_uri(&locations.jdt_data)?;
    let data_index = launch
        .config
        .args
        .iter()
        .position(|arg| arg == "-data")
        .ok_or_else(|| invalid("Java launch is missing its fixed data argument"))?;
    let argument = launch
        .config
        .args
        .get_mut(data_index + 1)
        .ok_or_else(|| invalid("Java launch is missing its fixed data location"))?;
    *argument = data_uri.into();
    launch.config.args.splice(
        0..0,
        [
            "-Djava.import.generatesMetadataFilesAtProjectRoot=false".into(),
            format!("-Duser.home={home}").into(),
            format!("-Djava.io.tmpdir={temporary}").into(),
        ],
    );
    launch.initialization_options = initialization_options(&pom_uri, &locations);
    launch.maven = Some(MavenSession {
        root,
        local_repository: repository,
        pom_sha256: format!("{:x}", Sha256::digest(&bytes)),
        pom_uri,
        declared_dependencies,
        source_paths: model.source_paths,
    });
    // JDT owns these files for the lifetime of its process. Persist only this new
    // directory; never reuse or overwrite a user's existing settings or data.
    let _persisted = controls.keep();
    Ok(launch)
}

pub(super) fn current_pom_matches(session: &MavenSession) -> Result<(), RemoteError> {
    let unchanged = (|| {
        existing_path(&session.root, true)?;
        reject_project_configuration(&session.root)?;
        let bytes = read_pom(&session.root)?;
        Ok::<bool, RemoteError>(format!("{:x}", Sha256::digest(&bytes)) == session.pom_sha256)
    })();
    if matches!(unchanged, Ok(true)) {
        Ok(())
    } else {
        Err(error(
            "language_maven_restart_required",
            "The selected Maven project configuration changed; stop and restart Java to import it",
        ))
    }
}

fn check_environment() -> Result<(), RemoteError> {
    java_launch::check_environment()?;
    for name in [
        "MAVEN_OPTS",
        "MAVEN_ARGS",
        "MAVEN_CONFIG",
        "MAVEN_USER_HOME",
        "M2_HOME",
        "MAVEN_HOME",
        "MAVEN_PROJECTBASEDIR",
        "MAVEN_CMD_LINE_ARGS",
        "MAVEN_EXT_CLASS_PATH",
    ] {
        if std::env::var_os(name).is_some() {
            return Err(invalid("Maven launch requires a clean Maven environment"));
        }
    }
    Ok(())
}

fn ascii_path(path: &Path) -> Result<&str, RemoteError> {
    path.to_str()
        .filter(|text| text.is_ascii() && !text.chars().any(char::is_control))
        .ok_or_else(|| invalid("Maven control data requires an ASCII path for the JDK launcher"))
}

fn ordinary_spelling(path: &Path) -> Result<(), RemoteError> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid("Maven paths must be UTF-8"))?;
    if !path.is_absolute() || text.len() > 4096 || text.chars().any(char::is_control) {
        return Err(invalid("Maven paths must be ordinary absolute local paths"));
    }
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
    {
        return Err(invalid("Maven requires ordinary local-drive paths"));
    }
    for part in path.components() {
        match part {
            Component::Normal(name) if name.to_str().is_some_and(safe_component) => (),
            Component::Prefix(_) | Component::RootDir => (),
            _ => return Err(invalid("Maven paths cannot contain aliases or traversal")),
        }
    }
    // Path::components normalizes internal '.', so inspect the spelling too.
    if text
        .split(['/', '\\'])
        .any(|part| matches!(part, "." | ".."))
    {
        return Err(invalid("Maven paths cannot contain aliases or traversal"));
    }
    Ok(())
}

fn existing_path(path: &Path, directory: bool) -> Result<PathBuf, RemoteError> {
    ordinary_spelling(path)?;
    for ancestor in path
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
    {
        java_launch::regular_path(ancestor, ancestor != path || directory)?;
    }
    java_launch::ordinary_local_path(path)
}

fn outside_project(root: &Path, path: &Path) -> Result<PathBuf, RemoteError> {
    let path = existing_path(path, true)?;
    if path
        .canonicalize()
        .map_err(io_error)?
        .starts_with(root.canonicalize().map_err(io_error)?)
    {
        return Err(invalid(
            "Maven cache and control data must already exist outside the project",
        ));
    }
    Ok(path)
}

fn reject_project_configuration(root: &Path) -> Result<(), RemoteError> {
    for name in [".mvn", ".project", ".classpath", ".settings"] {
        match fs::symlink_metadata(root.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io_error(error)),
            Ok(_) => return Err(invalid(
                "Project Maven or Eclipse configuration is unsupported by the leaf Maven profile",
            )),
        }
    }
    Ok(())
}

fn read_pom(root: &Path) -> Result<Vec<u8>, RemoteError> {
    let path = root.join("pom.xml");
    existing_path(&path, false)?;
    let file = File::open(path).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.len() > MAX_POM_BYTES as u64 {
        return Err(invalid("The root pom.xml exceeds the 128 KiB limit"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_POM_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_POM_BYTES {
        return Err(invalid("The root pom.xml exceeds the 128 KiB limit"));
    }
    Ok(bytes)
}

fn safe_component(value: &str) -> bool {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || value.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return false;
    }
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) && !(stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        && !matches!(
            stem.as_str(),
            "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
        )
}

fn relative_path(value: &str) -> Result<String, RemoteError> {
    if value.is_empty()
        || value.len() > 4096
        || value.contains("${")
        || !value.split(['/', '\\']).all(safe_component)
    {
        return Err(invalid(
            "Maven build paths must be confined literal relative paths",
        ));
    }
    let normalized = value.replace('\\', "/");
    if normalized.split('/').any(|part| part.starts_with('.')) {
        return Err(invalid(
            "Maven build paths cannot select hidden project configuration",
        ));
    }
    Ok(normalized)
}

/// An absent tail is intentional: offline JDT may report unresolved artifacts.
/// Every component which already exists is still checked without following links.
fn confined_path(root: &Path, relative: &str, directory: bool) -> Result<PathBuf, RemoteError> {
    let relative = relative_path(relative)?;
    let parts: Vec<_> = relative.split('/').collect();
    let mut path = root.to_path_buf();
    let mut absent = false;
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        if absent {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => absent = true,
            Err(error) => return Err(io_error(error)),
            Ok(_) => java_launch::regular_path(&path, index + 1 != parts.len() || directory)?,
        }
    }
    Ok(path)
}

#[derive(Debug)]
struct Node {
    name: String,
    text: String,
    children: Vec<Node>,
}

fn element(start: &quick_xml::events::BytesStart<'_>, root: bool) -> Result<Node, RemoteError> {
    let name = std::str::from_utf8(start.name().as_ref())
        .map_err(|_| invalid("The root POM must be UTF-8 XML"))?
        .to_owned();
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(invalid("The POM contains an unsupported XML element name"));
    }
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| invalid("Malformed POM XML attributes"))?;
        let accepted = root
            && match attribute.key.as_ref() {
                b"xmlns" => attribute.value.as_ref() == b"http://maven.apache.org/POM/4.0.0",
                b"xmlns:xsi" => {
                    attribute.value.as_ref() == b"http://www.w3.org/2001/XMLSchema-instance"
                }
                b"xsi:schemaLocation" => matches!(attribute.value.as_ref(),
                b"http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd"
                | b"http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd"),
                _ => false,
            };
        if !accepted {
            return Err(invalid("The POM contains unsupported XML attributes"));
        }
    }
    Ok(Node {
        name,
        text: String::new(),
        children: Vec::new(),
    })
}

fn xml_tree(bytes: &[u8]) -> Result<Node, RemoteError> {
    if bytes.len() > MAX_POM_BYTES {
        return Err(invalid("The root pom.xml exceeds the 128 KiB limit"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("The root POM must be UTF-8 XML"))?;
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(invalid("The root POM contains XML control characters"));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_comments = true;
    reader.config_mut().check_end_names = true;
    let mut stack: Vec<Node> = Vec::new();
    let mut document = None;
    let mut nodes = 0;
    let mut declaration_allowed = true;
    for _ in 0..MAX_XML_EVENTS {
        let event = reader
            .read_event()
            .map_err(|_| invalid("Malformed root POM XML"))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                if stack.is_empty() && document.is_some() {
                    return Err(invalid("The POM must contain exactly one project element"));
                }
                nodes += 1;
                if nodes > MAX_XML_NODES || stack.len() >= MAX_XML_DEPTH {
                    return Err(invalid("The POM exceeds XML node or nesting limits"));
                }
                let node = element(&start, stack.is_empty())?;
                if empty {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else {
                        document = Some(node);
                    }
                } else {
                    stack.push(node);
                }
                declaration_allowed = false;
            }
            Event::End(_) => {
                let node = stack
                    .pop()
                    .ok_or_else(|| invalid("Unbalanced root POM XML"))?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    document = Some(node);
                }
                declaration_allowed = false;
            }
            Event::Text(text) => {
                let value = std::str::from_utf8(text.as_ref())
                    .map_err(|_| invalid("The root POM must be UTF-8 XML"))?;
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(value);
                } else if !value.trim().is_empty() {
                    return Err(invalid("Text outside the root POM project element"));
                }
                declaration_allowed = false;
            }
            Event::Decl(declaration) if declaration_allowed => {
                validate_declaration(&declaration)?;
                declaration_allowed = false;
            }
            Event::Comment(_) => {
                declaration_allowed = false;
            }
            Event::Eof => {
                if !stack.is_empty() {
                    return Err(invalid("Unclosed POM element"));
                }
                return document.ok_or_else(|| invalid("The root POM is empty"));
            }
            _ => {
                return Err(invalid(
                    "POM entities, DTDs, CDATA, and processing instructions are unsupported",
                ))
            }
        }
    }
    Err(invalid("The POM exceeds the XML event limit"))
}

fn validate_declaration(declaration: &quick_xml::events::BytesDecl<'_>) -> Result<(), RemoteError> {
    let text = std::str::from_utf8(declaration.as_ref())
        .map_err(|_| invalid("Invalid UTF-8 XML declaration"))?;
    let start = quick_xml::events::BytesStart::from_content(text, 3);
    let mut phase = 0;
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| invalid("Invalid XML declaration attributes"))?;
        let next = match attribute.key.as_ref() {
            b"version" if phase == 0 && attribute.value.as_ref() == b"1.0" => 1,
            b"encoding" if phase == 1 && attribute.value.eq_ignore_ascii_case(b"UTF-8") => 2,
            b"standalone"
                if matches!(phase, 1 | 2) && matches!(attribute.value.as_ref(), b"yes" | b"no") =>
            {
                3
            }
            _ => {
                return Err(invalid(
                    "Only a strict XML 1.0 UTF-8 declaration is supported",
                ))
            }
        };
        phase = next;
    }
    if phase == 0 {
        return Err(invalid("The XML declaration is missing its version"));
    }
    Ok(())
}

fn children(node: &Node, allowed: &[&str], repeated: Option<&str>) -> Result<(), RemoteError> {
    if !node.text.trim().is_empty() {
        return Err(invalid("Unexpected text in a POM container"));
    }
    let mut seen = BTreeSet::new();
    for child in &node.children {
        if !allowed.contains(&child.name.as_str()) {
            return Err(invalid(
                "The POM contains configuration outside the supported leaf Maven profile",
            ));
        }
        if repeated != Some(child.name.as_str()) && !seen.insert(child.name.as_str()) {
            return Err(invalid("The POM contains duplicate configuration elements"));
        }
    }
    Ok(())
}

fn child<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    node.children.iter().find(|child| child.name == name)
}

fn literal(node: &Node) -> Result<&str, RemoteError> {
    let text = node.text.trim();
    if !node.children.is_empty() || text.is_empty() || text.len() > 4096 || text.contains("${") {
        return Err(invalid(
            "POM values must be bounded, nonempty literals without interpolation",
        ));
    }
    Ok(text)
}

fn value<'a>(node: &'a Node, name: &str) -> Result<Option<&'a str>, RemoteError> {
    child(node, name).map(literal).transpose()
}

fn coordinate(value: &str, group: bool) -> Result<String, RemoteError> {
    if value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        || !safe_component(value)
        || (group && !value.split('.').all(safe_component))
        || value.eq_ignore_ascii_case("LATEST")
        || value.eq_ignore_ascii_case("RELEASE")
    {
        return Err(invalid(
            "Maven coordinates must be fixed literal path-safe values",
        ));
    }
    Ok(value.to_owned())
}

fn required_coordinate(node: &Node, name: &str, group: bool) -> Result<String, RemoteError> {
    coordinate(
        value(node, name)?.ok_or_else(|| invalid("The POM is missing a required coordinate"))?,
        group,
    )
}

#[derive(Debug)]
struct Dependency {
    group: String,
    artifact: String,
    version: String,
    classifier: Option<String>,
}

impl Dependency {
    fn repository_path(&self) -> String {
        let classifier = self
            .classifier
            .as_ref()
            .map(|value| format!("-{value}"))
            .unwrap_or_default();
        format!(
            "{}/{}/{}/{}-{}{}.jar",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            self.artifact,
            self.version,
            classifier
        )
    }
}

#[derive(Debug)]
struct Pom {
    dependencies: Vec<Dependency>,
    source_paths: Vec<String>,
    output_paths: Vec<String>,
}

fn parse_pom(bytes: &[u8]) -> Result<Pom, RemoteError> {
    let root = xml_tree(bytes)?;
    if root.name != "project" {
        return Err(invalid("The root XML element must be project"));
    }
    children(
        &root,
        &[
            "modelVersion",
            "groupId",
            "artifactId",
            "version",
            "packaging",
            "properties",
            "dependencies",
            "build",
        ],
        None,
    )?;
    if value(&root, "modelVersion")? != Some("4.0.0") {
        return Err(invalid("Only Maven modelVersion 4.0.0 is supported"));
    }
    required_coordinate(&root, "groupId", true)?;
    required_coordinate(&root, "artifactId", false)?;
    required_coordinate(&root, "version", false)?;
    if !matches!(value(&root, "packaging")?, None | Some("jar")) {
        return Err(invalid("Only leaf JAR packaging is supported"));
    }
    if let Some(properties) = child(&root, "properties") {
        children(
            properties,
            &[
                "maven.compiler.source",
                "maven.compiler.target",
                "maven.compiler.release",
                "project.build.sourceEncoding",
                "project.reporting.outputEncoding",
            ],
            None,
        )?;
        for property in &properties.children {
            let text = literal(property)?;
            if property.name.ends_with("Encoding") {
                if !matches!(text, "UTF-8" | "UTF-16" | "US-ASCII" | "ISO-8859-1") {
                    return Err(invalid("Unsupported literal Maven source encoding"));
                }
            } else {
                let number = text.strip_prefix("1.").unwrap_or(text);
                if !number.bytes().all(|b| b.is_ascii_digit())
                    || !number
                        .parse::<u16>()
                        .is_ok_and(|number| (1..=99).contains(&number))
                {
                    return Err(invalid(
                        "Compiler source, target, and release must be literal Java versions",
                    ));
                }
            }
        }
    }
    let mut dependencies = Vec::new();
    if let Some(container) = child(&root, "dependencies") {
        children(container, &["dependency"], Some("dependency"))?;
        if container.children.len() > MAX_DEPENDENCIES {
            return Err(invalid("The POM exceeds the direct dependency limit"));
        }
        let mut seen = BTreeSet::new();
        for dependency in &container.children {
            children(
                dependency,
                &[
                    "groupId",
                    "artifactId",
                    "version",
                    "scope",
                    "optional",
                    "classifier",
                    "type",
                ],
                None,
            )?;
            if !matches!(value(dependency, "type")?, None | Some("jar"))
                || !matches!(
                    value(dependency, "scope")?,
                    None | Some("compile" | "provided" | "runtime" | "test")
                )
                || !matches!(
                    value(dependency, "optional")?,
                    None | Some("true" | "false")
                )
            {
                return Err(invalid(
                    "Only literal direct JAR dependencies with ordinary scopes are supported",
                ));
            }
            let parsed = Dependency {
                group: required_coordinate(dependency, "groupId", true)?,
                artifact: required_coordinate(dependency, "artifactId", false)?,
                version: required_coordinate(dependency, "version", false)?,
                classifier: value(dependency, "classifier")?
                    .map(|value| coordinate(value, false))
                    .transpose()?,
            };
            if !seen.insert((
                parsed.group.clone(),
                parsed.artifact.clone(),
                parsed.classifier.clone(),
            )) {
                return Err(invalid(
                    "Duplicate Maven dependency coordinates are unsupported",
                ));
            }
            dependencies.push(parsed);
        }
    }
    let mut source_paths = vec!["src/main/java".into(), "src/test/java".into()];
    let mut output_paths = vec!["target/classes".into(), "target/test-classes".into()];
    if let Some(build) = child(&root, "build") {
        children(
            build,
            &[
                "sourceDirectory",
                "testSourceDirectory",
                "outputDirectory",
                "testOutputDirectory",
            ],
            None,
        )?;
        for (index, name) in ["sourceDirectory", "testSourceDirectory"]
            .into_iter()
            .enumerate()
        {
            if let Some(text) = value(build, name)? {
                source_paths[index] = relative_path(text)?;
            }
        }
        for (index, name) in ["outputDirectory", "testOutputDirectory"]
            .into_iter()
            .enumerate()
        {
            if let Some(text) = value(build, name)? {
                output_paths[index] = relative_path(text)?;
            }
        }
    }
    Ok(Pom {
        dependencies,
        source_paths,
        output_paths,
    })
}

struct ControlPaths {
    home: PathBuf,
    temporary: PathBuf,
    jdt_data: PathBuf,
    user_settings: PathBuf,
    global_settings: PathBuf,
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn settings_xml(repository: &Path, mirror: &Path) -> Result<String, RemoteError> {
    let repository = repository
        .to_str()
        .ok_or_else(|| invalid("Maven cache path must be UTF-8"))?;
    let mirror_uri = java_launch::directory_uri(mirror)?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<settings xmlns=\"http://maven.apache.org/SETTINGS/1.0.0\"><offline>true</offline><interactiveMode>false</interactiveMode><localRepository>{}</localRepository><mirrors><mirror><id>cedar-owned-file-only</id><mirrorOf>*</mirrorOf><url>{}</url></mirror></mirrors></settings>\n",
        xml_escape(repository), xml_escape(&mirror_uri),
    ))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), RemoteError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.flush().map_err(io_error)
}

fn create_controls(root: &Path, repository: &Path) -> Result<ControlPaths, RemoteError> {
    ascii_path(root)?;
    let paths = ControlPaths {
        home: root.join("home"),
        temporary: root.join("tmp"),
        jdt_data: root.join("jdt-data"),
        user_settings: root.join("user-settings.xml"),
        global_settings: root.join("global-settings.xml"),
    };
    let mirror = root.join("empty-mirror");
    for directory in [&paths.home, &paths.temporary, &paths.jdt_data, &mirror] {
        fs::create_dir(directory).map_err(io_error)?;
    }
    let settings = settings_xml(repository, &mirror)?;
    write_new(&paths.user_settings, settings.as_bytes())?;
    write_new(&paths.global_settings, settings.as_bytes())?;
    Ok(paths)
}

fn initialization_options(pom_uri: &str, paths: &ControlPaths) -> Value {
    json!({
        "projectConfigurations": [pom_uri],
        "settings": { "java": {
            "search": {"scope": "all"},
            "import": {
                "maven": {"enabled": true, "offline": {"enabled": true}},
                "gradle": {"enabled": false, "wrapper": {"enabled": false}, "offline": {"enabled": true}}
            },
            "maven": {"downloadSources": false, "updateSnapshots": false},
            "eclipse": {"downloadSources": false},
            "autobuild": {"enabled": false},
            "maxConcurrentBuilds": 1,
            "configuration": {
                "updateBuildConfiguration": "disabled",
                "maven": {"userSettings": paths.user_settings, "globalSettings": paths.global_settings}
            }
        }}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAUNCH_ENVIRONMENT: &[&str] = &[
        "CLIENT_PORT",
        "CLIENT_HOST",
        "socket.stream.debug",
        "JDK_JAVA_OPTIONS",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "MAVEN_OPTS",
        "MAVEN_ARGS",
        "MAVEN_CONFIG",
        "MAVEN_USER_HOME",
        "M2_HOME",
        "MAVEN_HOME",
        "MAVEN_PROJECTBASEDIR",
        "MAVEN_CMD_LINE_ARGS",
        "MAVEN_EXT_CLASS_PATH",
    ];
    const CHILD_MODE: &str = "CEDAR_MAVEN_UNIT_CHILD_MODE";
    const CHILD_COMPLETED: &str = "CEDAR_MAVEN_UNIT_COMPLETED_V1";

    fn completed_child_output(output: &[u8]) -> bool {
        std::str::from_utf8(output).is_ok_and(|text| {
            text.lines().filter(|line| *line == CHILD_COMPLETED).count() == 1
                && text.contains("running 1 test")
                && text.contains("test result: ok. 1 passed; 0 failed; 0 ignored")
        })
    }

    fn isolated_test(test: &str, mode: &str, injected_name: Option<&str>) {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", test, "--test-threads=1", "--nocapture"])
            .env(CHILD_MODE, mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in LAUNCH_ENVIRONMENT {
            command.env_remove(name);
        }
        if let Some(name) = injected_name {
            command.env(name, "cedar-owned-environment-test");
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("isolated Maven recipe test exceeded its deadline");
            }
            if child.try_wait().unwrap().is_some() {
                let output = child.wait_with_output().unwrap();
                assert!(
                    Instant::now() < deadline,
                    "isolated Maven test completed too late"
                );
                assert!(
                    output.stdout.len() <= 32768 && output.stderr.len() <= 32768,
                    "isolated Maven test output exceeded its bound"
                );
                assert!(output.status.success(), "isolated Maven test failed");
                assert!(
                    completed_child_output(&output.stdout),
                    "isolated Maven test did not prove one completed case"
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn environment_guard_rejects_each_injection_without_parent_mutation() {
        if let Some(mode) = std::env::var_os(CHILD_MODE) {
            assert!(
                mode == "clean" || mode == "injected",
                "invalid isolated test mode"
            );
            assert_eq!(check_environment().is_ok(), mode == "clean");
            println!("\n{CHILD_COMPLETED}");
            return;
        }
        let before: Vec<_> = LAUNCH_ENVIRONMENT.iter().map(std::env::var_os).collect();
        let test =
            "java_maven::tests::environment_guard_rejects_each_injection_without_parent_mutation";
        isolated_test(test, "clean", None);
        for name in LAUNCH_ENVIRONMENT {
            isolated_test(test, "injected", Some(name));
        }
        assert!(
            before
                == LAUNCH_ENVIRONMENT
                    .iter()
                    .map(std::env::var_os)
                    .collect::<Vec<_>>(),
            "parent launcher environment changed"
        );
    }

    #[test]
    fn child_completion_requires_one_actual_test_and_one_witness() {
        let good = format!(
            "running 1 test\n{CHILD_COMPLETED}\ntest result: ok. 1 passed; 0 failed; 0 ignored\n"
        );
        assert!(completed_child_output(good.as_bytes()));
        for invalid in [
            good.replace(CHILD_COMPLETED, ""),
            good.replace("running 1 test", "running 0 tests"),
            good.replace("1 passed", "0 passed"),
            format!("{good}{CHILD_COMPLETED}\n"),
        ] {
            assert!(!completed_child_output(invalid.as_bytes()));
        }
        assert!(!completed_child_output(&[255]));
    }

    fn pom(body: &str) -> Vec<u8> {
        format!("<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>leaf</artifactId><version>1.0</version>{body}</project>").into_bytes()
    }

    #[test]
    fn parser_accepts_bounded_literal_leaf_and_missing_artifact() {
        let parsed = parse_pom(&pom("<properties><maven.compiler.release>17</maven.compiler.release><project.build.sourceEncoding>UTF-8</project.build.sourceEncoding></properties><dependencies><dependency><groupId>org.example</groupId><artifactId>api</artifactId><version>1.2.3</version><classifier>tests</classifier><scope>test</scope><optional>true</optional></dependency></dependencies><build><sourceDirectory>source/雪</sourceDirectory></build>")).unwrap();
        assert_eq!(parsed.source_paths, ["source/雪", "src/test/java"]);
        let root = tempfile::tempdir().unwrap();
        let relative = parsed.dependencies[0].repository_path();
        assert_eq!(relative, "org/example/api/1.2.3/api-1.2.3-tests.jar");
        let missing = confined_path(root.path(), &relative, false).unwrap();
        assert!(!missing.exists());
        assert!(missing.starts_with(root.path()));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn parser_rejects_unsupported_execution_and_model_features() {
        for reserved in [
            "COM¹",
            "COM².jar",
            "COM³",
            "LPT¹",
            "LPT².jar",
            "LPT³",
            "com¹",
        ] {
            assert!(
                !safe_component(reserved),
                "accepted Windows device {reserved}"
            );
        }
        for body in [
            "<parent/>", "<modules/>", "<profiles/>", "<dependencyManagement/>", "<repositories/>",
            "<pluginRepositories/>", "<build><plugins/></build>", "<build><extensions/></build>",
            "<build><resources/></build>", "<properties><arbitrary>anything</arbitrary></properties>",
            "<dependencies><dependency><groupId>org.example</groupId><artifactId>x</artifactId><version>${env.VERSION}</version></dependency></dependencies>",
            "<dependencies><dependency><groupId>org.example</groupId><artifactId>x</artifactId><version>[1,2)</version></dependency></dependencies>",
            "<dependencies><dependency><groupId>org.example</groupId><artifactId>x</artifactId><version>1</version><systemPath>x.jar</systemPath></dependency></dependencies>",
            "<build><sourceDirectory>../outside</sourceDirectory></build>",
            "<build><outputDirectory>C:\\outside</outputDirectory></build>",
            "<build><outputDirectory>target/CON</outputDirectory></build>",
            "<groupId>duplicate</groupId>",
        ] {
            assert!(parse_pom(&pom(body)).is_err(), "accepted {body}");
        }
        for text in [
            "<!DOCTYPE project [<!ENTITY x 'value'>]><project/>",
            "<project>&amp;</project>",
            "<project><![CDATA[value]]></project>",
            "<?arbitrary value?><project/>",
            "<project/><project/>",
            "<project><x></project>",
            "<project xmlns='https://unrecognized.invalid'/>",
        ] {
            assert!(parse_pom(text.as_bytes()).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn xml_size_depth_and_node_limits_are_finite() {
        assert!(xml_tree(&vec![b' '; MAX_POM_BYTES + 1]).is_err());
        let deep = format!(
            "{}{}",
            "<x>".repeat(MAX_XML_DEPTH + 1),
            "</x>".repeat(MAX_XML_DEPTH + 1)
        );
        assert!(xml_tree(deep.as_bytes()).is_err());
        let many = format!("<project>{}</project>", "<x/>".repeat(MAX_XML_NODES));
        assert!(xml_tree(many.as_bytes()).is_err());
        assert!(parse_pom(&pom("")).is_ok());
        let namespaced = String::from_utf8(pom("")).unwrap().replacen(
            "<project>",
            "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><project xmlns='http://maven.apache.org/POM/4.0.0' xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance' xsi:schemaLocation='http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd'>",
            1,
        );
        assert!(parse_pom(namespaced.as_bytes()).is_ok());
        for declaration in [
            "<?xml version='1.0' version='1.1'?>",
            "<?xml version='1.0' encoding='UTF-16'?>",
            "<?xml version='1.0' custom='value'?>",
        ] {
            let text = format!("{declaration}{}", String::from_utf8(pom("")).unwrap());
            assert!(parse_pom(text.as_bytes()).is_err());
        }
    }

    #[test]
    fn settings_escape_unicode_cache_and_never_overwrite_existing_files() {
        assert_eq!(xml_escape("雪<&>\"'"), "雪&lt;&amp;&gt;&quot;&apos;");
        let root = tempfile::tempdir().unwrap();
        let control = root.path().join("control");
        fs::create_dir(&control).unwrap();
        let cache = root.path().join("cache 雪 & friends");
        fs::create_dir(&cache).unwrap();
        let paths = create_controls(&control, &cache).unwrap();
        let settings = fs::read_to_string(&paths.user_settings).unwrap();
        assert!(settings.contains("cache 雪 &amp; friends"));
        assert!(settings.contains("<offline>true</offline>"));
        assert!(settings.contains("<mirrorOf>*</mirrorOf>"));
        assert!(settings.contains("<url>file:"));
        assert_eq!(
            settings,
            fs::read_to_string(&paths.global_settings).unwrap()
        );
        assert!(write_new(&paths.user_settings, b"overwrite").is_err());
        assert_eq!(settings, fs::read_to_string(&paths.user_settings).unwrap());
        assert_eq!(fs::read_dir(&paths.home).unwrap().count(), 0);
        let config = initialization_options("file:///project/pom.xml", &paths);
        assert_eq!(
            config["projectConfigurations"],
            json!(["file:///project/pom.xml"])
        );
        assert_eq!(
            config["settings"]["java"]["import"]["maven"]["offline"]["enabled"],
            true
        );
        assert_eq!(
            config["settings"]["java"]["import"]["gradle"]["wrapper"]["enabled"],
            false
        );
        assert_eq!(config["settings"]["java"]["autobuild"]["enabled"], false);
        assert_eq!(
            config["settings"]["java"]["configuration"]["updateBuildConfiguration"],
            "disabled"
        );
    }

    #[cfg(windows)]
    #[test]
    fn production_keeps_unicode_locations_and_fresh_ascii_controls() {
        if std::env::var_os(CHILD_MODE).as_deref() != Some(std::ffi::OsStr::new("recipe")) {
            isolated_test(
                "java_maven::tests::production_keeps_unicode_locations_and_fresh_ascii_controls",
                "recipe",
                None,
            );
            return;
        }
        fn argument(launch: &java_launch::JavaLaunch, flag: &str) -> String {
            let index = launch
                .config
                .args
                .iter()
                .position(|value| value == flag)
                .unwrap();
            launch.config.args[index + 1].to_str().unwrap().to_owned()
        }
        fn user_settings(launch: &java_launch::JavaLaunch) -> PathBuf {
            PathBuf::from(
                launch.initialization_options["settings"]["java"]["configuration"]["maven"]
                    ["userSettings"]
                    .as_str()
                    .unwrap(),
            )
        }

        let fixture = tempfile::tempdir().unwrap();
        let base = java_launch::ordinary_local_path(fixture.path()).unwrap();
        // This launch profile explicitly requires an ASCII JDK/control parent.
        // A machine with a Unicode-only temporary location cannot supply it.
        assert!(
            base.to_str().is_some_and(str::is_ascii),
            "Maven recipe fixture requires an ASCII temporary parent"
        );
        let root = base.join("project 雪");
        let cache = base.join("cache 雪 & jars");
        let distribution = base.join("distribution 雪");
        let data = base.join("control data");
        let java = base.join("java.exe");
        for directory in [&root, &cache, &data, &distribution] {
            fs::create_dir(directory).unwrap();
        }
        fs::create_dir(distribution.join("config_win")).unwrap();
        fs::create_dir(distribution.join("plugins")).unwrap();
        fs::write(
            distribution.join("plugins/org.eclipse.equinox.launcher_1.jar"),
            b"fixture",
        )
        .unwrap();
        fs::write(&java, b"fixture executable, never launched").unwrap();
        fs::write(root.join("pom.xml"), pom("")).unwrap();
        let sentinel = data.join("user-settings.xml");
        fs::write(&sentinel, b"existing user settings").unwrap();
        // Workspace passes its internal canonical Windows verbatim-disk root.
        let canonical_root = root.canonicalize().unwrap();
        let prepare = |control_data: &Path| {
            production(
                &canonical_root,
                java.to_str().unwrap(),
                distribution.to_str().unwrap(),
                control_data.to_str().unwrap(),
                cache.to_str().unwrap(),
            )
        };
        let first = prepare(&data).unwrap();
        assert_eq!(first.config.program.as_os_str(), java.as_os_str());
        assert_eq!(
            first.config.working_directory.as_ref().unwrap(),
            &java_launch::ordinary_local_path(&distribution).unwrap()
        );
        assert!(first
            .config
            .args
            .iter()
            .all(|value| value.to_str().is_some_and(str::is_ascii)));
        assert_eq!(
            PathBuf::from(argument(&first, "-jar")),
            Path::new("plugins").join("org.eclipse.equinox.launcher_1.jar")
        );
        assert_eq!(
            argument(&first, "-configuration"),
            java_launch::directory_uri(&distribution.join("config_win")).unwrap()
        );
        let first_settings = user_settings(&first);
        let first_control = first_settings.parent().unwrap();
        let first_data = url::Url::parse(&argument(&first, "-data"))
            .unwrap()
            .to_file_path()
            .unwrap();
        assert_eq!(first_data, first_control.join("jdt-data"));
        assert!(first_control.starts_with(&data));
        assert!(!first_control.starts_with(&root));
        assert!(first_control
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("cedar-maven-"));
        assert!(first_data.is_dir());
        for (property, directory) in [("-Duser.home=", "home"), ("-Djava.io.tmpdir=", "tmp")] {
            let expected = format!("{property}{}", first_control.join(directory).display());
            assert!(expected.is_ascii());
            assert!(first
                .config
                .args
                .iter()
                .any(|value| value == expected.as_str()));
            assert!(first_control.join(directory).is_dir());
        }
        assert!(!first
            .config
            .args
            .iter()
            .any(|value| value.to_str().unwrap().starts_with("-Dmaven.repo.local=")));
        let settings = fs::read_to_string(&first_settings).unwrap();
        assert!(settings.contains(&xml_escape(cache.to_str().unwrap())));
        assert!(settings.contains("<id>cedar-owned-file-only</id>"));
        let session = first.maven.as_ref().unwrap();
        assert_eq!(
            session.root,
            java_launch::ordinary_local_path(&root).unwrap()
        );
        assert_eq!(
            session.local_repository,
            java_launch::ordinary_local_path(&cache).unwrap()
        );
        assert_eq!(
            first.initialization_options["projectConfigurations"],
            json!([session.pom_uri])
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        fs::write(&first_settings, b"keep first session settings").unwrap();
        let second = prepare(&data).unwrap();
        let second_settings = user_settings(&second);
        assert_ne!(first_settings, second_settings);
        assert_ne!(argument(&first, "-data"), argument(&second, "-data"));
        assert_eq!(
            fs::read(&first_settings).unwrap(),
            b"keep first session settings"
        );
        assert_eq!(fs::read(&sentinel).unwrap(), b"existing user settings");
        assert!(fs::read_to_string(&second_settings)
            .unwrap()
            .contains("<offline>true</offline>"));

        let unicode_data = base.join("uncreated controls 雪");
        let before = fs::read_dir(&data).unwrap().count();
        assert_eq!(
            prepare(&unicode_data).err().unwrap().code,
            "invalid_java_maven"
        );
        assert!(!unicode_data.exists());
        assert_eq!(fs::read_dir(&data).unwrap().count(), before);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        println!("\n{CHILD_COMPLETED}");
    }

    #[cfg(unix)]
    #[test]
    fn ancestors_and_existing_dependency_components_cannot_be_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::create_dir(real.join("child")).unwrap();
        std::os::unix::fs::symlink(&real, root.path().join("alias")).unwrap();
        assert!(existing_path(&root.path().join("alias/child"), true).is_err());
        assert!(confined_path(root.path(), "alias/missing.jar", false).is_err());
        assert!(confined_path(root.path(), "../escape.jar", false).is_err());
    }

    #[test]
    fn changed_or_missing_pom_requires_restart() {
        let directory = tempfile::tempdir().unwrap();
        let root = java_launch::ordinary_local_path(directory.path()).unwrap();
        let bytes = pom("");
        fs::write(root.join("pom.xml"), &bytes).unwrap();
        let session = MavenSession {
            root: root.clone(),
            local_repository: root.clone(),
            pom_sha256: format!("{:x}", Sha256::digest(&bytes)),
            pom_uri: "file:///project/pom.xml".into(),
            declared_dependencies: Vec::new(),
            source_paths: Vec::new(),
        };
        current_pom_matches(&session).unwrap();
        fs::write(root.join("pom.xml"), pom("<packaging>jar</packaging>")).unwrap();
        assert_eq!(
            current_pom_matches(&session).unwrap_err().code,
            "language_maven_restart_required"
        );
        fs::remove_file(root.join("pom.xml")).unwrap();
        assert_eq!(
            current_pom_matches(&session).unwrap_err().code,
            "language_maven_restart_required"
        );
    }
}
