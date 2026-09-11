//! Adopting definitions persists them, lists their identity and refuses local changes.

use crate::*;

#[test]
fn project_adopt_persists_definitions_lists_identity_and_refuses_local_changes() {
    let destination = destination_repository();
    let source = source_repository();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            source.path().join("packages/tui/tests/support.mjs"),
            fs::Permissions::from_mode(0o751),
        )
        .expect("distinct source mode");
    }
    let source_text = source.path().to_str().expect("UTF-8 source path");

    let adopted = run(
        destination.path(),
        &["project", "adopt", "--source", source_text],
    );
    assert!(
        adopted.status.success(),
        "{}",
        String::from_utf8_lossy(&adopted.stderr)
    );
    let adopted = json_output(&adopted);
    assert_eq!(
        adopted["schema"],
        "ai.wisent.probierz.project-adoption-result.v1"
    );
    assert_eq!(adopted["status"], "imported");
    assert_eq!(
        adopted["sourceRoot"],
        fs::canonicalize(source.path())
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(adopted["applications"], serde_json::json!(["example"]));
    assert_eq!(adopted["imported"], 3);
    assert_eq!(adopted["unchanged"], 0);
    assert_eq!(adopted["removed"], 0);
    assert_eq!(adopted["conflicting"], 0);
    assert_eq!(adopted["rejected"], 0);
    assert_eq!(adopted["conflicts"], serde_json::json!([]));
    assert_eq!(
        adopted["skippedLocalState"],
        serde_json::json!(["apps/.adoptions.json"])
    );
    assert_eq!(adopted["executedJourneys"], false);
    assert_eq!(
        fs::read(destination.path().join("apps/example/probierz.yaml")).unwrap(),
        fs::read(source.path().join("apps/example/probierz.yaml")).unwrap()
    );
    assert_eq!(
        fs::read(
            destination
                .path()
                .join("packages/tui/tests/example.spec.mjs")
        )
        .unwrap(),
        fs::read(source.path().join("packages/tui/tests/example.spec.mjs")).unwrap()
    );
    assert_ne!(
        fs::read(destination.path().join("apps/.adoptions.json")).unwrap(),
        fs::read(source.path().join("apps/.adoptions.json")).unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(destination.path().join("packages/tui/tests/support.mjs"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o751
        );
    }
    assert!(!destination.path().join("test-results").exists());

    let index_file = destination.path().join("apps/.adoptions.json");
    let index: Value =
        serde_json::from_slice(&fs::read(&index_file).unwrap()).expect("adoption index");
    assert_eq!(index["schema"], "ai.wisent.probierz.project-adoptions.v1");
    assert_eq!(index["sources"].as_array().map(Vec::len), Some(1));
    assert_eq!(index["sources"][0]["sourceDigest"], adopted["sourceDigest"]);
    assert_eq!(
        index["sources"][0]["files"].as_array().map(Vec::len),
        Some(3)
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&index_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let listed = run(destination.path(), &["project", "adoptions"]);
    assert!(listed.status.success());
    let listed = json_output(&listed);
    assert_eq!(listed["schema"], "ai.wisent.probierz.project-adoptions.v1");
    assert_eq!(
        listed["file"],
        fs::canonicalize(destination.path())
            .unwrap()
            .join("apps/.adoptions.json")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(listed["sources"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["sources"][0]["sourceRoot"], adopted["sourceRoot"]);
    assert_eq!(
        listed["sources"][0]["sourceDigest"],
        adopted["sourceDigest"]
    );
    assert_eq!(
        listed["sources"][0]["applications"],
        serde_json::json!(["example"])
    );
    assert_eq!(listed["sources"][0]["fileCount"], 3);
    assert!(listed["sources"][0]["adoptedAt"]
        .as_str()
        .is_some_and(|value| value.ends_with('Z')));
    assert!(listed["sources"][0].get("files").is_none());

    // The documented duplicate behavior is an unchanged success, not a new copy.
    let duplicate = run(
        destination.path(),
        &["project", "adopt", "--source", source_text],
    );
    assert!(duplicate.status.success());
    let duplicate = json_output(&duplicate);
    assert_eq!(duplicate["status"], "unchanged");
    assert_eq!(duplicate["imported"], 0);
    assert_eq!(duplicate["unchanged"], 3);

    let source_helper = source.path().join("packages/tui/tests/support.mjs");
    let adopted_helper = destination.path().join("packages/tui/tests/support.mjs");
    fs::remove_file(&source_helper).expect("remove upstream helper");
    let removal_refused = run(
        destination.path(),
        &["project", "adopt", "--source", source_text],
    );
    assert_eq!(removal_refused.status.code(), Some(1));
    let removal_refused = json_output(&removal_refused);
    assert_eq!(
        removal_refused["conflicts"][0]["path"],
        "packages/tui/tests/support.mjs"
    );
    assert_eq!(
        removal_refused["conflicts"][0]["reason"],
        "previously adopted definition is absent from the selected source"
    );
    assert!(adopted_helper.is_file());

    let removed = run(
        destination.path(),
        &["project", "adopt", "--source", source_text, "--replace"],
    );
    assert!(removed.status.success());
    let removed = json_output(&removed);
    assert_eq!(removed["status"], "replaced");
    assert_eq!(removed["imported"], 0);
    assert_eq!(removed["unchanged"], 2);
    assert_eq!(removed["removed"], 1);
    assert!(!adopted_helper.exists());

    let adopted_spec = destination
        .path()
        .join("packages/tui/tests/example.spec.mjs");
    fs::write(&adopted_spec, "locally reviewed edit\n").expect("local change");
    let index_before_refusal = fs::read(&index_file).expect("index before refusal");
    for arguments in [
        vec!["project", "adopt", "--source", source_text],
        vec!["project", "adopt", "--source", source_text, "--replace"],
    ] {
        let refused = run(destination.path(), &arguments);
        assert_eq!(refused.status.code(), Some(1));
        assert!(refused.stderr.is_empty());
        let refused = json_output(&refused);
        assert_eq!(refused["status"], "conflict");
        assert_eq!(refused["imported"], 0);
        assert_eq!(refused["conflicting"], 1);
        assert_eq!(refused["rejected"], 1);
        assert_eq!(
            refused["conflicts"][0]["path"],
            "packages/tui/tests/example.spec.mjs"
        );
        assert_eq!(
            refused["conflicts"][0]["reason"],
            "previously adopted definition has local content or mode changes"
        );
        assert_eq!(
            fs::read_to_string(&adopted_spec).unwrap(),
            "locally reviewed edit\n"
        );
        assert_eq!(fs::read(&index_file).unwrap(), index_before_refusal);
    }
}
