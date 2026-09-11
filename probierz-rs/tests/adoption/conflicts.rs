//! Conflicts are atomic, replacement is explicit, and another source always conflicts.

use crate::*;

#[test]
fn conflicts_are_atomic_and_reviewed_replacement_is_explicit() {
    let destination = destination_repository();
    let source = source_repository();
    let source_text = source.path().to_str().expect("UTF-8 source path");
    fs::create_dir_all(destination.path().join("apps/example")).expect("existing app directory");
    let conflicting_file = destination.path().join("apps/example/probierz.yaml");
    fs::write(&conflicting_file, "keep this unmanaged definition\n").expect("existing definition");
    fs::create_dir_all(destination.path().join("packages/tui/tests"))
        .expect("existing spec directory");
    let conflicting_spec = destination
        .path()
        .join("packages/tui/tests/example.spec.mjs");
    fs::write(&conflicting_spec, "keep this unmanaged spec\n").expect("existing spec");

    let refused = run(
        destination.path(),
        &["project", "adopt", "--source", source_text],
    );
    assert_eq!(refused.status.code(), Some(1));
    let refused = json_output(&refused);
    assert_eq!(refused["status"], "conflict");
    assert_eq!(refused["conflicting"], 2);
    assert_eq!(refused["rejected"], 2);
    let conflicts = refused["conflicts"]
        .as_array()
        .expect("complete conflict list");
    for path in [
        "apps/example/probierz.yaml",
        "packages/tui/tests/example.spec.mjs",
    ] {
        let conflict = conflicts
            .iter()
            .find(|conflict| conflict["path"] == path)
            .unwrap_or_else(|| panic!("missing conflict for {path}"));
        assert_eq!(
            conflict["reason"],
            "destination content or mode differs; repeat with explicit replacement"
        );
    }
    assert_eq!(
        fs::read_to_string(&conflicting_file).unwrap(),
        "keep this unmanaged definition\n"
    );
    assert_eq!(
        fs::read_to_string(&conflicting_spec).unwrap(),
        "keep this unmanaged spec\n"
    );
    assert!(!destination
        .path()
        .join("packages/tui/tests/support.mjs")
        .exists());
    assert!(!destination.path().join("apps/.adoptions.json").exists());

    let replaced = run(
        destination.path(),
        &["project", "adopt", "--source", source_text, "--replace"],
    );
    assert!(
        replaced.status.success(),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    let replaced = json_output(&replaced);
    assert_eq!(replaced["status"], "imported");
    assert_eq!(replaced["imported"], 3);
    assert_eq!(
        fs::read(&conflicting_file).unwrap(),
        fs::read(source.path().join("apps/example/probierz.yaml")).unwrap()
    );
    assert_eq!(
        fs::read(&conflicting_spec).unwrap(),
        fs::read(source.path().join("packages/tui/tests/example.spec.mjs")).unwrap()
    );
}

#[test]
fn another_source_always_conflicts_even_when_every_definition_is_identical() {
    let destination = destination_repository();
    let first = source_repository();
    let first_text = first.path().to_str().expect("UTF-8 first source");
    let adopted = run(
        destination.path(),
        &["project", "adopt", "--source", first_text],
    );
    assert!(adopted.status.success());

    let second = source_with_identical_definitions(first.path());
    let second_text = second.path().to_str().expect("UTF-8 second source");
    let refused = run(
        destination.path(),
        &["project", "adopt", "--source", second_text, "--replace"],
    );
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stderr.is_empty());
    let refused = json_output(&refused);
    assert_eq!(refused["status"], "conflict");
    assert_eq!(refused["unchanged"], 0);
    assert_eq!(refused["conflicting"], 3);
    assert_eq!(refused["rejected"], 3);
    assert!(refused["conflicts"]
        .as_array()
        .expect("complete conflict list")
        .iter()
        .all(|conflict| conflict["reason"] == "destination is owned by another adopted source"));

    let listed = json_output(&run(destination.path(), &["project", "adoptions"]));
    assert_eq!(listed["sources"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        listed["sources"][0]["sourceRoot"],
        fs::canonicalize(first.path())
            .expect("canonical first source")
            .to_string_lossy()
            .as_ref()
    );
}
