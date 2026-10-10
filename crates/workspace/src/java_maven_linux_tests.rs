//! Production Maven preparation only: the executable is an ELF-shaped file,
//! never a Java process, and environment changes stay in isolated test children.
use super::tests::{isolated_test, pom, CHILD_COMPLETED, CHILD_MODE};
use super::*;
use std::ffi::OsStr;
use std::os::unix::fs::{symlink, PermissionsExt};

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    java: PathBuf,
    distribution: PathBuf,
    data: PathBuf,
    cache: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let base = temporary.path().canonicalize().unwrap();
        assert!(base.to_str().unwrap().is_ascii());
        let root = base.join("project 雪");
        let java = base.join("JDK 雪/bin/java");
        let distribution = base.join("distribution 雪");
        let data = base.join("control data");
        let cache = base.join("cache 雪 & jars");
        for directory in [&root, &distribution, &data, &cache] {
            fs::create_dir(directory).unwrap();
        }
        fs::create_dir_all(java.parent().unwrap()).unwrap();
        fs::write(&java, b"\x7fELFfixture").unwrap();
        fs::set_permissions(&java, fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir(distribution.join("config_linux")).unwrap();
        fs::create_dir(distribution.join("plugins")).unwrap();
        fs::write(
            distribution.join("plugins/org.eclipse.equinox.launcher_1.jar"),
            b"fixture",
        )
        .unwrap();
        fs::write(root.join("pom.xml"), pom("")).unwrap();
        fs::write(data.join("user-settings.xml"), b"user-owned settings").unwrap();
        Self {
            _temporary: temporary,
            root,
            java,
            distribution,
            data,
            cache,
        }
    }

    fn prepare(&self) -> Result<java_launch::JavaLaunch, RemoteError> {
        self.prepare_with(&self.java, &self.distribution, &self.data, &self.cache)
    }

    fn prepare_with(
        &self,
        java: &Path,
        distribution: &Path,
        data: &Path,
        cache: &Path,
    ) -> Result<java_launch::JavaLaunch, RemoteError> {
        production(
            &self.root,
            java.to_str().unwrap(),
            distribution.to_str().unwrap(),
            data.to_str().unwrap(),
            cache.to_str().unwrap(),
        )
    }

    fn assert_no_controls(&self) {
        assert_eq!(fs::read_dir(&self.data).unwrap().count(), 1);
        assert_eq!(
            fs::read(self.data.join("user-settings.xml")).unwrap(),
            b"user-owned settings"
        );
    }
}

fn clean_recipe(test: &str) -> bool {
    if std::env::var_os(CHILD_MODE).as_deref() == Some(OsStr::new("linux-recipe")) {
        true
    } else {
        isolated_test(test, "linux-recipe", None);
        false
    }
}

#[test]
fn linux_maven_requires_config_linux_without_windows_fallback() {
    if !clean_recipe(
        "java_maven::linux_tests::linux_maven_requires_config_linux_without_windows_fallback",
    ) {
        return;
    }
    let fixture = Fixture::new();
    let linux = fixture.distribution.join("config_linux");
    let windows = fixture.distribution.join("config_win");
    fs::rename(&linux, &windows).unwrap();
    assert!(fixture.prepare().is_err());
    fixture.assert_no_controls();
    fs::create_dir(&linux).unwrap();
    let launch = fixture.prepare().unwrap();
    let index = launch
        .config
        .args
        .iter()
        .position(|argument| argument == "-configuration")
        .unwrap();
    assert_eq!(
        launch.config.args[index + 1],
        OsStr::new(&java_launch::directory_uri(&linux).unwrap())
    );
    assert_eq!(launch.config.program.as_os_str(), fixture.java.as_os_str());
    println!("\n{CHILD_COMPLETED}");
}

#[test]
fn linux_maven_controls_require_existing_ascii_directory_outside_workspace() {
    if !clean_recipe(
        "java_maven::linux_tests::linux_maven_controls_require_existing_ascii_directory_outside_workspace",
    ) {
        return;
    }
    let mut fixture = Fixture::new();
    // ASCII project paths ensure the containment guard itself is exercised,
    // independently of the control parent's ASCII restriction.
    let ascii_root = fixture.root.parent().unwrap().join("project");
    fs::rename(&fixture.root, &ascii_root).unwrap();
    fixture.root = ascii_root;
    let base = fixture.data.parent().unwrap();
    let unicode_data = base.join("controls 雪");
    let missing_data = base.join("missing controls");
    let file_data = base.join("control-file");
    let in_project = fixture.root.join("controls");
    fs::create_dir(&unicode_data).unwrap();
    fs::create_dir(&in_project).unwrap();
    fs::write(&file_data, b"not a directory").unwrap();
    for data in [
        &unicode_data,
        &missing_data,
        &file_data,
        &fixture.root,
        &in_project,
    ] {
        assert!(fixture
            .prepare_with(&fixture.java, &fixture.distribution, data, &fixture.cache)
            .is_err());
        fixture.assert_no_controls();
    }
    assert!(!missing_data.exists());
    assert!(fs::read_dir(&unicode_data).unwrap().next().is_none());
    assert!(fs::read_dir(&in_project).unwrap().next().is_none());
    for cache in [&fixture.root, &in_project, &missing_data] {
        assert!(fixture
            .prepare_with(&fixture.java, &fixture.distribution, &fixture.data, cache)
            .is_err());
        fixture.assert_no_controls();
    }
    println!("\n{CHILD_COMPLETED}");
}

#[test]
fn linux_maven_rejects_selected_paths_and_ancestors_that_are_aliases() {
    if !clean_recipe(
        "java_maven::linux_tests::linux_maven_rejects_selected_paths_and_ancestors_that_are_aliases",
    ) {
        return;
    }
    let fixture = Fixture::new();
    let base = fixture.data.parent().unwrap();
    // Plain Java permits a selected JDK parent symlink. Maven deliberately
    // retains its stricter no-alias policy for every existing ancestor.
    let java_parent_alias = base.join("selected JDK");
    symlink(fixture.java.parent().unwrap(), &java_parent_alias).unwrap();
    let selected_java = java_parent_alias.join("java");
    let java_file_alias = base.join("java");
    symlink(&fixture.java, &java_file_alias).unwrap();
    for java in [&selected_java, &java_file_alias] {
        assert!(fixture
            .prepare_with(java, &fixture.distribution, &fixture.data, &fixture.cache)
            .is_err());
    }
    let distribution_alias = base.join("selected distribution");
    let data_alias = base.join("selected controls");
    let cache_alias = base.join("selected cache");
    symlink(&fixture.distribution, &distribution_alias).unwrap();
    symlink(&fixture.data, &data_alias).unwrap();
    symlink(&fixture.cache, &cache_alias).unwrap();
    for (distribution, data, cache) in [
        (&distribution_alias, &fixture.data, &fixture.cache),
        (&fixture.distribution, &data_alias, &fixture.cache),
        (&fixture.distribution, &fixture.data, &cache_alias),
    ] {
        assert!(fixture
            .prepare_with(&fixture.java, distribution, data, cache)
            .is_err());
    }
    let parent_alias = base.join("selected parent");
    symlink(base, &parent_alias).unwrap();
    for (distribution, data, cache) in [
        (
            parent_alias.join("distribution 雪"),
            fixture.data.clone(),
            fixture.cache.clone(),
        ),
        (
            fixture.distribution.clone(),
            parent_alias.join("control data"),
            fixture.cache.clone(),
        ),
        (
            fixture.distribution.clone(),
            fixture.data.clone(),
            parent_alias.join("cache 雪 & jars"),
        ),
    ] {
        assert!(fixture
            .prepare_with(&fixture.java, &distribution, &data, &cache)
            .is_err());
    }
    for relative in [
        "config_linux",
        "plugins",
        "plugins/org.eclipse.equinox.launcher_1.jar",
    ] {
        let path = fixture.distribution.join(relative);
        let original = base.join("saved distribution entry");
        fs::rename(&path, &original).unwrap();
        symlink(&original, &path).unwrap();
        assert!(fixture.prepare().is_err(), "{relative}");
        fs::remove_file(&path).unwrap();
        fs::rename(&original, &path).unwrap();
    }
    for path in [
        base.join("distribution 雪/./plugins"),
        base.join("distribution 雪/../distribution 雪"),
        base.join("distribution 雪\n"),
    ] {
        assert!(fixture
            .prepare_with(&fixture.java, &path, &fixture.data, &fixture.cache)
            .is_err());
    }
    fixture.assert_no_controls();
    println!("\n{CHILD_COMPLETED}");
}

#[test]
fn linux_maven_validates_leaf_pom_paths_and_keeps_original_pom_hash() {
    if !clean_recipe(
        "java_maven::linux_tests::linux_maven_validates_leaf_pom_paths_and_keeps_original_pom_hash",
    ) {
        return;
    }
    let fixture = Fixture::new();
    let pom_path = fixture.root.join("pom.xml");
    for body in [
        "<parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1</version></parent>",
        "<modules><module>child</module></modules>",
        "<build><sourceDirectory>../outside</sourceDirectory></build>",
        "<build><outputDirectory>/outside</outputDirectory></build>",
    ] {
        fs::write(&pom_path, pom(body)).unwrap();
        assert!(fixture.prepare().is_err());
        fixture.assert_no_controls();
    }
    fs::write(&pom_path, pom("")).unwrap();
    for configuration in [".mvn", ".project", ".classpath", ".settings"] {
        let path = fixture.root.join(configuration);
        fs::write(&path, b"fixture").unwrap();
        assert!(fixture.prepare().is_err());
        fs::remove_file(path).unwrap();
        fixture.assert_no_controls();
    }
    let saved_pom = fixture.data.parent().unwrap().join("saved-pom.xml");
    fs::rename(&pom_path, &saved_pom).unwrap();
    symlink(&saved_pom, &pom_path).unwrap();
    assert!(fixture.prepare().is_err());
    fs::remove_file(&pom_path).unwrap();
    fs::rename(saved_pom, &pom_path).unwrap();
    for relative in ["src", "target"] {
        let alias = fixture.root.join(relative);
        symlink(&fixture.cache, &alias).unwrap();
        assert!(fixture.prepare().is_err());
        fs::remove_file(alias).unwrap();
        fixture.assert_no_controls();
    }
    let body = "<dependencies><dependency><groupId>org.example</groupId><artifactId>api</artifactId><version>1.2.3</version></dependency></dependencies><build><sourceDirectory>source/雪</sourceDirectory></build>";
    let bytes = pom(body);
    fs::write(&pom_path, &bytes).unwrap();
    let artifact = fixture.cache.join("org/example/api/1.2.3/api-1.2.3.jar");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    symlink(&pom_path, &artifact).unwrap();
    assert!(fixture.prepare().is_err());
    fs::remove_file(&artifact).unwrap();
    fixture.assert_no_controls();
    // Missing cached artifacts remain valid and are reported as unresolved.
    let launch = fixture.prepare().unwrap();
    let session = launch.maven.as_ref().unwrap();
    assert_eq!(session.pom_sha256, format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(session.source_paths, ["source/雪", "src/test/java"]);
    assert_eq!(session.declared_dependencies, [artifact]);
    assert_eq!(session.declarations.len(), 1);
    assert_eq!(
        url::Url::parse(&session.pom_uri)
            .unwrap()
            .to_file_path()
            .unwrap(),
        pom_path
    );
    current_pom_matches(session).unwrap();
    fs::write(&pom_path, pom("")).unwrap();
    assert_eq!(
        current_pom_matches(session).unwrap_err().code,
        "language_maven_restart_required"
    );
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 1);
    println!("\n{CHILD_COMPLETED}");
}
