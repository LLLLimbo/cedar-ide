//! The narrow installed Windows JDT launch recipe, shared with acceptance fixtures.
//! Host paths are explicit; this module never downloads, creates data directories,
//! changes environment variables, searches PATH, or executes a shell.
use crate::{error, io_error};
use cedar_language::{ClientOptions, ProcessConfig};
use cedar_protocol::RemoteError;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};

fn invalid(message: &'static str) -> RemoteError {
    error("invalid_java_launch", message)
}

pub(super) struct JavaLaunch {
    pub config: ProcessConfig,
    pub options: ClientOptions,
    pub initialization_options: Value,
    pub maven: Option<crate::java_maven::MavenSession>,
}

pub(super) fn production(
    workspace_root: &Path,
    java_executable: &str,
    distribution: &str,
    data_directory: &str,
) -> Result<JavaLaunch, RemoteError> {
    for text in [java_executable, distribution, data_directory] {
        if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control) {
            return Err(invalid(
                "Java host paths must contain 1..4096 bytes without control characters",
            ));
        }
    }
    validate_java_executable(std::ffi::OsStr::new(java_executable))?;
    let (distribution, launcher, configuration_uri) =
        validate_distribution(Path::new(distribution))?;
    let data = production_data_directory(workspace_root, Path::new(data_directory))?;
    check_environment()?;
    let data_uri = directory_uri(&data)?;
    let mut config = ProcessConfig::new(java_executable);
    config.working_directory = Some(distribution);
    config.args = jvm_arguments();
    config.args.extend([
        "-jar".into(),
        launcher.into(),
        "-configuration".into(),
        configuration_uri.into(),
        "-data".into(),
        data_uri.into(),
    ]);
    Ok(JavaLaunch {
        config,
        options: client_options(),
        initialization_options: initialization_options(),
        maven: None,
    })
}

pub(super) fn production_data_directory(root: &Path, data: &Path) -> Result<PathBuf, RemoteError> {
    if !data.is_absolute() {
        return Err(invalid(
            "Java data directory must be an existing absolute path outside the workspace",
        ));
    }
    regular_path(data, true)?;
    let data = ordinary_local_path(data)?;
    let canonical_data = data.canonicalize().map_err(io_error)?;
    let canonical_root = root.canonicalize().map_err(io_error)?;
    if canonical_data.starts_with(&canonical_root) {
        return Err(invalid(
            "Java data directory must be outside the workspace, not the project root or its child",
        ));
    }
    Ok(data)
}

pub(super) fn jvm_arguments() -> Vec<std::ffi::OsString> {
    [
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
    .collect()
}

pub(super) fn client_options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(60),
        shutdown_timeout: Duration::from_secs(10),
        ..ClientOptions::default()
    }
}

pub(super) fn initialization_options() -> Value {
    // Cedar has no class-file content viewer. Only the validation wrapper may
    // advertise that extended capability to exercise its fixed JDK witness.
    json!({"settings":{"java":{"search":{"scope":"all"},"import":{"gradle":{"enabled":false},"maven":{"enabled":false}}}}})
}

pub(super) fn check_environment() -> Result<(), RemoteError> {
    for name in [
        "CLIENT_PORT",
        "CLIENT_HOST",
        "socket.stream.debug",
        "JDK_JAVA_OPTIONS",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
    ] {
        if std::env::var_os(name).is_some() {
            return Err(invalid("Java launch requires a clean launcher environment"));
        }
    }
    Ok(())
}

pub(super) fn directory_uri(path: &Path) -> Result<String, RemoteError> {
    url::Url::from_directory_path(path)
        .map(String::from)
        .map_err(|_| invalid("Cannot encode Java location as a local directory URL"))
}

pub(super) fn regular_path(path: &Path, directory: bool) -> Result<(), RemoteError> {
    let meta = fs::symlink_metadata(path).map_err(io_error)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if meta.file_type().is_symlink()
        || reparse
        || if directory {
            !meta.is_dir()
        } else {
            !meta.is_file()
        }
    {
        return Err(invalid(
            "Java launch paths must be ordinary files or directories",
        ));
    }
    Ok(())
}

pub(super) fn ordinary_local_path(path: &Path) -> Result<PathBuf, RemoteError> {
    let canonical = path.canonicalize().map_err(io_error)?;
    #[cfg(windows)]
    let ordinary = {
        use std::path::Prefix;
        let text = canonical
            .to_str()
            .ok_or_else(|| invalid("Java launch paths must be UTF-8"))?;
        let plain = match canonical.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(_) => text
                    .strip_prefix(r"\\?\")
                    .ok_or_else(|| invalid("Invalid local-drive path"))?,
                Prefix::Disk(_) if canonical.is_absolute() => text,
                _ => return Err(invalid("Java launch requires a local-drive path")),
            },
            _ => return Err(invalid("Java launch requires an absolute local-drive path")),
        };
        PathBuf::from(plain)
    };
    #[cfg(not(windows))]
    let ordinary = canonical.clone();
    if ordinary.canonicalize().map_err(io_error)? != canonical {
        return Err(invalid(
            "Ordinary Java path changed the selected filesystem identity",
        ));
    }
    Ok(ordinary)
}

pub(super) fn validate_distribution(
    path: &Path,
) -> Result<(PathBuf, PathBuf, String), RemoteError> {
    if !path.is_absolute() {
        return Err(invalid("Java distribution must be absolute"));
    }
    regular_path(path, true)?;
    let distribution = ordinary_local_path(path)?;
    let configuration = distribution.join("config_win");
    regular_path(&configuration, true)?;
    let plugins = distribution.join("plugins");
    regular_path(&plugins, true)?;
    let mut launcher = None;
    for (index, entry) in fs::read_dir(&plugins).map_err(io_error)?.enumerate() {
        if index >= 4096 {
            return Err(invalid("Java plugins directory exceeds launch limit"));
        }
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name();
        if name.to_str().is_some_and(|name| {
            name.starts_with("org.eclipse.equinox.launcher_") && name.ends_with(".jar")
        }) {
            if launcher.is_some() {
                return Err(invalid("Expected exactly one Equinox launcher JAR"));
            }
            regular_path(&entry.path(), false)?;
            launcher = Some(entry.path());
        }
    }
    let launcher = launcher.ok_or_else(|| invalid("Expected exactly one Equinox launcher JAR"))?;
    let relative = relative_launcher(&distribution, &launcher)?;
    let configuration_uri = url::Url::from_directory_path(&configuration)
        .map_err(|_| invalid("Invalid Java configuration URI"))?
        .into();
    Ok((distribution, relative, configuration_uri))
}

pub(super) fn relative_launcher(
    distribution: &Path,
    launcher: &Path,
) -> Result<PathBuf, RemoteError> {
    let relative = launcher
        .strip_prefix(distribution)
        .map_err(|_| invalid("Launcher must be inside its distribution"))?;
    if relative.as_os_str().is_empty()
        || !relative.to_str().is_some_and(str::is_ascii)
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || distribution
            .join(relative)
            .canonicalize()
            .map_err(io_error)?
            != launcher.canonicalize().map_err(io_error)?
    {
        return Err(invalid("Launcher requires an exact ASCII relative path"));
    }
    Ok(relative.to_path_buf())
}

pub(super) fn ordinary_ascii_java_spelling(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.is_ascii()
        && !bytes.iter().any(u8::is_ascii_control)
        && bytes.len() > 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
        && text[3..].split(['/', '\\']).all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part.contains(':')
        })
        && text
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("java.exe"))
}

pub(super) fn validate_java_executable(program: &std::ffi::OsStr) -> Result<(), RemoteError> {
    if !program.to_str().is_some_and(ordinary_ascii_java_spelling) {
        return Err(invalid(
            "Java launch requires an ordinary absolute ASCII java.exe",
        ));
    }
    let path = Path::new(program);
    regular_path(path, false)?;
    if ordinary_local_path(path)?
        .canonicalize()
        .map_err(io_error)?
        != path.canonicalize().map_err(io_error)?
    {
        return Err(invalid("Java executable identity changed"));
    }
    // Deliberately do not replace ProcessConfig.program with its canonical path.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_initialization_never_claims_a_class_file_content_viewer() {
        let value = initialization_options();
        assert!(value.get("extendedClientCapabilities").is_none());
        // Standard workspace/symbol uses the existing indexed type scope. It
        // does not enable method search, importers or a class-file viewer.
        assert_eq!(value["settings"]["java"]["search"]["scope"], "all");
        assert!(value["settings"]["java"]["symbols"]
            .get("includeSourceMethodDeclarations")
            .is_none());
        assert_eq!(
            value["settings"]["java"]["import"]["maven"]["enabled"],
            false
        );
        assert_eq!(
            value["settings"]["java"]["import"]["gradle"]["enabled"],
            false
        );
        let options = client_options();
        assert_eq!(options.request_timeout, Duration::from_secs(60));
        assert_eq!(options.shutdown_timeout, Duration::from_secs(10));
        assert_eq!(options.outbound_capacity, 64);
        assert_eq!(options.event_capacity, 256);
        assert_eq!(options.max_pending_requests, 128);
        // The shared helper cannot change ordinary LSP defaults.
        assert_eq!(
            ClientOptions::default().request_timeout,
            Duration::from_secs(10)
        );
        assert_eq!(
            ClientOptions::default().shutdown_timeout,
            Duration::from_secs(1)
        );
    }

    #[test]
    fn production_data_is_existing_absolute_and_outside_the_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project 雪");
        let external = temp.path().join("JDT data 雪");
        fs::create_dir_all(project.join("inside")).unwrap();
        fs::create_dir(&external).unwrap();
        assert_eq!(
            production_data_directory(&project, &external)
                .unwrap()
                .canonicalize()
                .unwrap(),
            external.canonicalize().unwrap()
        );
        for rejected in [
            project.clone(),
            project.join("inside"),
            temp.path().join("missing"),
            PathBuf::from("relative"),
        ] {
            assert!(
                production_data_directory(&project, &rejected).is_err(),
                "{rejected:?}"
            );
        }
        assert!(!temp.path().join("missing").exists());
    }

    #[cfg(unix)]
    #[test]
    fn production_data_rejects_aliases_without_writing_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let data = temp.path().join("data");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir(&data).unwrap();
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&data, &alias).unwrap();
        assert!(production_data_directory(&root, &alias).is_err());
        assert!(!data.join(".metadata").exists());
    }

    #[test]
    fn fixed_jvm_options_do_not_expose_fixture_hooks_or_arbitrary_commands() {
        let args = jvm_arguments();
        assert!(args.contains(&"-Xmx512m".into()));
        assert!(args.contains(&"-Declipse.application=org.eclipse.jdt.ls.core.id1".into()));
        assert_eq!(args.len(), 10);
        assert!(args
            .iter()
            .all(|arg| !arg.to_string_lossy().contains("ErrorFile")
                && !arg.to_string_lossy().contains("cedar-windows")));
    }

    #[cfg(windows)]
    #[test]
    fn production_recipe_keeps_literal_java_and_unicode_distribution_locations() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let data = temp.path().join("data with spaces 雪");
        let distribution = temp.path().join("distribution with spaces 雪");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(distribution.join("config_win")).unwrap();
        fs::create_dir_all(distribution.join("plugins")).unwrap();
        fs::write(
            distribution.join("plugins/org.eclipse.equinox.launcher_1.jar"),
            b"fixture",
        )
        .unwrap();
        let java = temp.path().join("java.exe");
        fs::write(&java, b"fixture").unwrap();
        let java = ordinary_local_path(&java).unwrap();
        let Some(java_text) = java.to_str().filter(|text| text.is_ascii()) else {
            return;
        };
        let launch = production(
            &root,
            java_text,
            distribution.to_str().unwrap(),
            data.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(launch.config.program.as_os_str(), java.as_os_str());
        assert_eq!(
            launch.config.working_directory.unwrap(),
            ordinary_local_path(&distribution).unwrap()
        );
        assert_eq!(
            launch.config.args[11],
            Path::new("plugins")
                .join("org.eclipse.equinox.launcher_1.jar")
                .as_os_str()
        );
        assert_eq!(
            launch.config.args[13],
            directory_uri(
                &ordinary_local_path(&distribution)
                    .unwrap()
                    .join("config_win")
            )
            .unwrap()
            .as_str()
        );
        assert_eq!(
            launch.config.args[15],
            directory_uri(&ordinary_local_path(&data).unwrap())
                .unwrap()
                .as_str()
        );
        assert!(launch
            .config
            .args
            .iter()
            .all(|arg| arg.to_str().unwrap().is_ascii()));
        assert!(!data.join(".metadata").exists());
        assert!(fs::read_dir(root).unwrap().next().is_none());
    }
}
