use super::*;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{symlink, PermissionsExt};

fn executable(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Validation identifies ELF without pretending to parse or execute it.
    fs::write(path, b"\x7fELFfixture").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn distribution(path: &Path, configuration: &str) {
    fs::create_dir_all(path.join(configuration)).unwrap();
    fs::create_dir(path.join("plugins")).unwrap();
    fs::write(
        path.join("plugins/org.eclipse.equinox.launcher_1.jar"),
        b"fixture",
    )
    .unwrap();
}

#[test]
fn linux_java_spelling_is_absolute_literal_unicode_and_bounded() {
    for accepted in [
        "/java",
        "/opt/JDK 21/bin/java",
        "/opt/工具 雪/bin/java",
        "/opt/e\u{0301}/bin/java",
        "/opt/%E9%9B%AA $HOME 'literal';/java",
    ] {
        assert!(ordinary_linux_java_spelling(accepted), "{accepted:?}");
    }
    for rejected in [
        "",
        "java",
        "bin/java",
        "./java",
        "~/bin/java",
        "/opt/bin/java.exe",
        "/opt/bin/JAVA",
        "/opt/bin/java.sh",
        "/opt/bin/java ",
        "/opt/bin/java/",
        "//opt/bin/java",
        "/opt//bin/java",
        "/opt/./bin/java",
        "/opt/bin/../java",
        "/opt/bin/java/../java",
        "/opt/\\bin/java",
        "C:/Java/bin/java",
        "file:///opt/bin/java",
        "/opt/\0/bin/java",
        "/opt/\n/bin/java",
        "/opt/\u{0085}/bin/java",
    ] {
        assert!(!ordinary_linux_java_spelling(rejected), "{rejected:?}");
    }
    let boundary = format!("/{}/java", "雪".repeat(1363) + "a");
    assert_eq!(boundary.len(), 4096);
    assert!(ordinary_linux_java_spelling(&boundary));
    assert!(!ordinary_linux_java_spelling(&format!("/a{boundary}")));
}

#[test]
fn linux_java_requires_regular_executable_elf_and_exact_basename() {
    let temp = tempfile::tempdir().unwrap();
    let java = temp.path().join("JDK 雪/bin/java");
    executable(&java);
    validate_linux_java_executable(java.as_os_str()).unwrap();

    fs::set_permissions(&java, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(validate_linux_java_executable(java.as_os_str()).is_err());
    fs::set_permissions(&java, fs::Permissions::from_mode(0o755)).unwrap();
    for contents in [
        b"#!/bin/sh\nexit 0\n".as_slice(),
        b"echo shell fallback".as_slice(),
        b"MZfixture".as_slice(),
        b"\x7fEL".as_slice(),
        b"".as_slice(),
    ] {
        fs::write(&java, contents).unwrap();
        assert!(validate_linux_java_executable(java.as_os_str()).is_err());
    }
    executable(&java);
    for name in ["java.exe", "java.sh", "JAVA", "other"] {
        let wrong = temp.path().join(name);
        executable(&wrong);
        assert!(validate_linux_java_executable(wrong.as_os_str()).is_err());
    }
    let directory = temp.path().join("directory/java");
    fs::create_dir_all(&directory).unwrap();
    assert!(validate_linux_java_executable(directory.as_os_str()).is_err());
    assert!(validate_linux_java_executable(temp.path().join("missing/java").as_os_str()).is_err());
    let alias = temp.path().join("alias/java");
    fs::create_dir(alias.parent().unwrap()).unwrap();
    symlink(&java, &alias).unwrap();
    assert!(validate_linux_java_executable(alias.as_os_str()).is_err());
    let invalid_utf8 = OsString::from_vec(b"/jdk/\xff/java".to_vec());
    assert!(validate_linux_java_executable(&invalid_utf8).is_err());
    assert!(validate_linux_java_executable(OsStr::new("java")).is_err());
}

#[test]
fn linux_java_preserves_parent_alias_identity_and_rejects_traversal_spelling() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("JDK 雪/bin/java");
    executable(&original);
    let parent_alias = temp.path().join("selected JDK 雪");
    symlink(original.parent().unwrap(), &parent_alias).unwrap();
    let selected = parent_alias.join("java");
    validate_linux_java_executable(selected.as_os_str()).unwrap();
    assert_eq!(
        ordinary_local_path(&selected).unwrap(),
        original.canonicalize().unwrap()
    );
    for path in [
        format!("{}/./java", parent_alias.display()),
        format!("{}//java", parent_alias.display()),
        format!("{}/../bin/java", original.parent().unwrap().display()),
    ] {
        assert!(validate_linux_java_executable(OsStr::new(&path)).is_err());
    }
}

#[test]
fn linux_production_selects_linux_configuration_and_preserves_literal_locations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project 雪");
    let data = temp.path().join("data 雪 % # e\u{0301}");
    let installation = temp.path().join("distribution 雪 % # e\u{0301}");
    let java = temp.path().join("JDK 雪 % $HOME 'literal';/bin/java");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&data).unwrap();
    distribution(&installation, "config_linux");
    executable(&java);
    let alias = temp.path().join("selected JDK 雪");
    symlink(java.parent().unwrap(), &alias).unwrap();
    let selected_java = alias.join("java");
    let launch = production(
        &root,
        selected_java.to_str().unwrap(),
        installation.to_str().unwrap(),
        data.to_str().unwrap(),
    )
    .unwrap();
    assert_eq!(launch.config.program.as_os_str(), selected_java.as_os_str());
    assert_eq!(
        launch.config.working_directory.as_deref(),
        Some(installation.canonicalize().unwrap().as_path())
    );
    assert_eq!(launch.config.args.len(), 16);
    assert_eq!(&launch.config.args[..10], jvm_arguments().as_slice());
    for (index, flag) in [(10, "-jar"), (12, "-configuration"), (14, "-data")] {
        assert_eq!(launch.config.args[index], OsStr::new(flag));
    }
    assert_eq!(
        launch.config.args[11],
        OsStr::new("plugins/org.eclipse.equinox.launcher_1.jar")
    );
    for (index, expected) in [(13, installation.join("config_linux")), (15, data.clone())] {
        let argument = launch.config.args[index].to_str().unwrap();
        assert!(argument.is_ascii());
        assert!(argument.contains("%E9%9B%AA"));
        assert!(argument.contains("%25"));
        assert!(argument.contains("%23"));
        assert!(argument.contains("e%CC%81"));
        assert!(argument.ends_with('/'));
        assert_eq!(
            url::Url::parse(argument).unwrap().to_file_path().unwrap(),
            expected.canonicalize().unwrap()
        );
    }
    assert_eq!(launch.options.request_timeout, Duration::from_secs(60));
    assert_eq!(launch.options.shutdown_timeout, Duration::from_secs(10));
    assert_eq!(launch.initialization_options, initialization_options());
    assert!(launch.maven.is_none());
    assert!(!data.join(".metadata").exists());
    assert!(fs::read_dir(&root).unwrap().next().is_none());
}

#[test]
fn linux_production_and_windows_validation_do_not_fall_back_to_each_others_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let java = temp.path().join("java");
    executable(&java);
    let windows = temp.path().join("windows distribution 雪");
    distribution(&windows, "config_win");
    let (_, _, uri) = validate_distribution(&windows).unwrap();
    assert!(uri.ends_with("/config_win/"));
    assert!(production_distribution(java.to_str().unwrap(), &windows).is_err());

    let linux = temp.path().join("linux distribution 雪");
    distribution(&linux, "config_linux");
    let (_, _, uri) = production_distribution(java.to_str().unwrap(), &linux).unwrap();
    assert!(uri.ends_with("/config_linux/"));
    assert!(validate_distribution(&linux).is_err());
    // The fixture executable contract remains Windows-only on this Linux host.
    assert!(validate_java_executable(java.as_os_str()).is_err());
    assert!(ordinary_ascii_java_spelling(r"C:\Java\bin\java.exe"));
    assert!(!ordinary_ascii_java_spelling("/opt/JDK 雪/bin/java"));
}

#[test]
fn linux_distribution_retains_exact_launcher_and_ordinary_path_requirements() {
    let temp = tempfile::tempdir().unwrap();
    let java = temp.path().join("java");
    executable(&java);
    let installation = temp.path().join("distribution");
    distribution(&installation, "config_linux");
    let validate = || production_distribution(java.to_str().unwrap(), &installation);
    validate().unwrap();
    let launcher = installation.join("plugins/org.eclipse.equinox.launcher_1.jar");
    let duplicate = installation.join("plugins/org.eclipse.equinox.launcher_2.jar");
    fs::write(&duplicate, b"fixture").unwrap();
    assert!(validate().is_err());
    fs::remove_file(duplicate).unwrap();
    let unicode_launcher = installation.join("plugins/org.eclipse.equinox.launcher_雪.jar");
    fs::rename(&launcher, &unicode_launcher).unwrap();
    assert!(validate().is_err());
    fs::remove_file(unicode_launcher).unwrap();
    assert!(validate().is_err());
    symlink(&java, &launcher).unwrap();
    assert!(validate().is_err());
    fs::remove_file(&launcher).unwrap();
    fs::write(&launcher, b"fixture").unwrap();
    let configuration = installation.join("config_linux");
    fs::remove_dir(&configuration).unwrap();
    let outside = temp.path().join("outside configuration");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, &configuration).unwrap();
    assert!(validate().is_err());
    assert!(production_distribution(java.to_str().unwrap(), Path::new("relative")).is_err());
}

#[test]
fn linux_production_rejects_unbounded_or_control_containing_host_paths_before_use() {
    let root = Path::new("/unused workspace");
    for rejected in [String::new(), "x".repeat(4097), "/path/\nvalue".into()] {
        for paths in [
            [rejected.as_str(), "/distribution", "/data"],
            ["/jdk/bin/java", rejected.as_str(), "/data"],
            ["/jdk/bin/java", "/distribution", rejected.as_str()],
        ] {
            let error = production(root, paths[0], paths[1], paths[2])
                .err()
                .unwrap();
            assert_eq!(error.code, "invalid_java_launch");
            assert_eq!(
                error.message,
                "Java host paths must contain 1..4096 bytes without control characters"
            );
        }
    }
}
