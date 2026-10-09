use cedar_protocol::*;
use serde_json::json;

fn declaration(artifact: &str) -> MavenDependencyDeclaration {
    let mut declaration = MavenDependencyDeclaration {
        group_id: "org.example".into(),
        artifact_id: artifact.into(),
        version: "1.2.3".into(),
        classifier: None,
        scope: MavenDependencyScope::Compile,
        scope_explicit: false,
        optional: false,
        optional_explicit: false,
        expected_jar_path: String::new(),
        regular_file_present: false,
    };
    declaration.expected_jar_path = declaration.repository_jar_path();
    declaration
}

fn snapshot() -> MavenDependenciesSnapshot {
    let declaration = declaration("Api");
    MavenDependenciesSnapshot {
        schema: 1,
        profile: "maven_leaf".into(),
        startup_id: 7,
        pom_path: "pom.xml".into(),
        pom_sha256: "a".repeat(64),
        observation: MavenDependencyObservation::Available {
            libraries: vec![MavenObservedLibrary {
                root: MavenLibraryRoot::LocalRepository,
                relative_path: declaration.expected_jar_path.clone(),
                regular_file_present: false,
                declaration_indices: vec![0],
            }],
        },
        declarations: vec![declaration],
    }
}

fn valid(snapshot: &MavenDependenciesSnapshot) -> bool {
    snapshot.validate_for(7, &"a".repeat(64), true).is_ok()
}

fn libraries(snapshot: &mut MavenDependenciesSnapshot) -> &mut Vec<MavenObservedLibrary> {
    match &mut snapshot.observation {
        MavenDependencyObservation::Available { libraries } => libraries,
        _ => panic!("available fixture"),
    }
}

#[test]
fn strict_wire_roundtrip_and_unknown_fields_are_rejected() {
    let original = snapshot();
    assert!(valid(&original));
    let value = serde_json::to_value(&original).unwrap();
    assert_eq!(
        MavenDependenciesSnapshot::parse_for(&value, 7, &"a".repeat(64), true).unwrap(),
        original
    );
    let payload = Payload::MavenDependencies {
        snapshot: original.clone(),
    };
    let wire = serde_json::to_value(payload).unwrap();
    assert_eq!(wire["type"], "maven_dependencies");
    assert!(matches!(
        serde_json::from_value::<Payload>(wire).unwrap(),
        Payload::MavenDependencies { .. }
    ));
    for pointer in [
        "",
        "/declarations/0",
        "/observation",
        "/observation/libraries/0",
    ] {
        let mut bad = value.clone();
        bad.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("foreign".into(), json!("C:/private/path"));
        assert!(
            MavenDependenciesSnapshot::parse_for(&bad, 7, &"a".repeat(64), true).is_err(),
            "{pointer}"
        );
    }
    let mut bad = value;
    bad["observation"]["status"] = json!("guessed");
    assert!(MavenDependenciesSnapshot::parse_for(&bad, 7, &"a".repeat(64), true).is_err());
}

#[test]
fn identity_defaults_and_expected_coordinate_path_are_strict() {
    let good = snapshot();
    assert!(good.validate_for(8, &"a".repeat(64), true).is_err());
    assert!(good.validate_for(7, &"b".repeat(64), true).is_err());
    for mutate in [
        |s: &mut MavenDependenciesSnapshot| s.schema = 2,
        |s: &mut MavenDependenciesSnapshot| s.profile = "generic".into(),
        |s: &mut MavenDependenciesSnapshot| s.startup_id = 0,
        |s: &mut MavenDependenciesSnapshot| s.pom_path = "other/pom.xml".into(),
        |s: &mut MavenDependenciesSnapshot| s.pom_sha256 = "A".repeat(64),
        |s: &mut MavenDependenciesSnapshot| s.declarations[0].scope = MavenDependencyScope::Test,
        |s: &mut MavenDependenciesSnapshot| s.declarations[0].optional = true,
        |s: &mut MavenDependenciesSnapshot| {
            s.declarations[0].expected_jar_path = "org/foreign.jar".into()
        },
        |s: &mut MavenDependenciesSnapshot| s.declarations[0].artifact_id = "../outside".into(),
        |s: &mut MavenDependenciesSnapshot| s.declarations.push(s.declarations[0].clone()),
    ] {
        let mut bad = good.clone();
        mutate(&mut bad);
        assert!(!valid(&bad));
    }
    let mut explicit = good;
    explicit.declarations[0].scope_explicit = true;
    explicit.declarations[0].scope = MavenDependencyScope::Test;
    explicit.declarations[0].optional_explicit = true;
    explicit.declarations[0].optional = true;
    assert!(valid(&explicit));
}

#[test]
fn associations_preserve_case_distinct_declarations_without_first_match_choice() {
    let mut collision = snapshot();
    collision.declarations.push(declaration("api"));
    libraries(&mut collision)[0].declaration_indices = vec![0, 1];
    assert!(valid(&collision));
    assert!(collision.validate_for(7, &"a".repeat(64), false).is_err());
    for indices in [
        vec![0],
        vec![1],
        vec![1, 0],
        vec![0, 0, 1],
        vec![0, 2],
        vec![256],
        vec![],
    ] {
        let mut bad = collision.clone();
        libraries(&mut bad)[0].declaration_indices = indices;
        assert!(!valid(&bad));
    }
    let mut distinct = collision;
    libraries(&mut distinct)[0].declaration_indices = vec![0];
    assert!(distinct.validate_for(7, &"a".repeat(64), false).is_ok());
}

#[test]
fn library_rows_must_be_unique_confined_and_have_all_exact_links() {
    let original = snapshot();
    for relative in [
        "C:/repo/api.jar",
        "/repo/api.jar",
        "../api.jar",
        "x/../api.jar",
        "x//api.jar",
        "x\\api.jar",
        "x/CON.jar",
        "x/with\ncontrol.jar",
        ".",
    ] {
        let mut bad = original.clone();
        libraries(&mut bad)[0].relative_path = relative.into();
        assert!(!valid(&bad), "{relative}");
    }
    let mut duplicate = original.clone();
    let mut row = libraries(&mut duplicate)[0].clone();
    row.relative_path = row.relative_path.to_ascii_lowercase();
    libraries(&mut duplicate).push(row);
    assert!(!valid(&duplicate));
    let mut workspace = original.clone();
    libraries(&mut workspace)[0].root = MavenLibraryRoot::Workspace;
    assert!(!valid(&workspace));
    libraries(&mut workspace)[0].declaration_indices.clear();
    assert!(valid(&workspace));
    let mut unmatched = original;
    libraries(&mut unmatched)[0].relative_path = "another/library.jar".into();
    libraries(&mut unmatched)[0].declaration_indices.clear();
    assert!(valid(&unmatched));
}

#[test]
fn unavailable_is_distinct_from_known_empty_and_retains_declarations() {
    let mut empty = snapshot();
    libraries(&mut empty).clear();
    assert!(valid(&empty));
    let mut unknown = empty.clone();
    unknown.observation = MavenDependencyObservation::Unavailable {
        reason: MavenDependencyUnavailableReason::ModelUnavailable,
    };
    unknown.declarations[0].regular_file_present = true;
    assert!(valid(&unknown));
    let wire = serde_json::to_value(unknown).unwrap();
    assert_eq!(
        wire["observation"],
        json!({"status":"unavailable", "reason":"model_unavailable"})
    );
    assert_ne!(
        wire["observation"],
        serde_json::to_value(empty).unwrap()["observation"]
    );
}

#[test]
fn limits_apply_to_lists_links_raw_json_and_final_serialization() {
    let mut max = snapshot();
    max.declarations = (0..MAX_MAVEN_DEPENDENCIES)
        .map(|index| declaration(&format!("a{index}")))
        .collect();
    libraries(&mut max).clear();
    assert!(valid(&max));
    let mut too_many = max.clone();
    too_many.declarations.push(declaration("extra"));
    assert!(!valid(&too_many));
    let mut rows = snapshot();
    *libraries(&mut rows) = (0..MAX_MAVEN_DEPENDENCIES)
        .map(|index| MavenObservedLibrary {
            root: MavenLibraryRoot::Workspace,
            relative_path: format!("lib/{index}.jar"),
            regular_file_present: false,
            declaration_indices: vec![],
        })
        .collect();
    assert!(valid(&rows));
    let row = libraries(&mut rows)[0].clone();
    libraries(&mut rows).push(row);
    assert!(!valid(&rows));
    let mut links = snapshot();
    libraries(&mut links)[0].declaration_indices = vec![0; MAX_MAVEN_DEPENDENCIES + 1];
    assert!(!valid(&links));
    let mut huge = max;
    for (index, declaration) in huge.declarations.iter_mut().enumerate() {
        declaration.artifact_id = format!("{index}{}", "a".repeat(250));
        declaration.expected_jar_path = declaration.repository_jar_path();
    }
    assert!(!valid(&huge));
    let mut raw = serde_json::to_value(snapshot()).unwrap();
    raw["extra"] = json!("x".repeat(MAX_MAVEN_DEPENDENCIES_BYTES));
    assert!(MavenDependenciesSnapshot::parse_for(&raw, 7, &"a".repeat(64), true).is_err());
}
