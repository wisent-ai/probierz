use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn repository(root: &Path) {
    fs::create_dir_all(root.join(".git")).expect("Git marker");
    fs::create_dir_all(root.join("apps")).expect("apps directory");
}

fn source_repository() -> TempDir {
    let source = tempfile::tempdir().expect("temporary source repository");
    repository(source.path());
    fs::create_dir_all(source.path().join("apps/example")).expect("app directory");
    fs::create_dir_all(source.path().join("packages/tui/tests")).expect("spec directory");
    fs::write(
        source.path().join("apps/.adoptions.json"),
        b"source-local adoption history is not a definition\n",
    )
    .expect("source-local adoption state");
    let root = serde_json::to_string(&source.path().to_string_lossy()).expect("YAML path string");
    fs::write(
        source.path().join("apps/example/probierz.yaml"),
        format!(
            "schemaVersion: 1\nappId: example\nowner: example maintainers\nrepositories:\n  - root: {root}\n    mappings: []\nsurfaces:\n  tui:\n    spec: example.spec.mjs\n    journeys: [smoke]\njourneys:\n  smoke:\n    owner: example maintainers\n    timeoutMs: 1000\n"
        ),
    )
    .expect("manifest");
    fs::write(
        source.path().join("packages/tui/tests/example.spec.mjs"),
        "describe('example', () => { it('smoke', () => {}); });\n",
    )
    .expect("journey spec");
    fs::write(
        source.path().join("packages/tui/tests/support.mjs"),
        "export const fixture = 'retained helper';\n",
    )
    .expect("journey helper");
    source
}

fn destination_repository() -> TempDir {
    let destination = tempfile::tempdir().expect("temporary destination repository");
    repository(destination.path());
    destination
}

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(arguments)
        .output()
        .expect("run probierz")
}

fn run_with_state(root: &Path, state: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(arguments)
        .env("XDG_STATE_HOME", state)
        .output()
        .expect("run probierz")
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "JSON stdout: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn project_adopt_persists_definitions_lists_identity_and_refuses_local_changes() {
    let destination = destination_repository();
    let source = source_repository();
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
fn invalid_selections_and_cli_shapes_are_refused_before_mutation() {
    let destination = destination_repository();
    let not_repository = tempfile::tempdir().expect("non-repository directory");
    let not_repository_text = not_repository.path().to_str().expect("UTF-8 path");
    let refused = run(
        destination.path(),
        &["project", "adopt", "--source", not_repository_text],
    );
    assert_eq!(refused.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains(&format!(
        "Adoption source is not a Git repository: {}",
        fs::canonicalize(not_repository.path()).unwrap().display()
    )));
    assert!(stderr.ends_with("Your request was refused; nothing ran.\n"));
    assert!(!destination.path().join("apps/.adoptions.json").exists());

    let same = run(
        destination.path(),
        &[
            "project",
            "adopt",
            "--source",
            destination.path().to_str().expect("UTF-8 destination"),
        ],
    );
    assert_eq!(same.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&same.stderr);
    assert!(stderr.contains("adoption source is already this Probierz project"));
    assert!(stderr.ends_with("Your request was refused; nothing ran.\n"));

    let source = source_repository();
    fs::remove_file(source.path().join("packages/tui/tests/example.spec.mjs"))
        .expect("remove declared spec");
    let missing_spec = run(
        destination.path(),
        &[
            "project",
            "adopt",
            "--source",
            source.path().to_str().expect("UTF-8 source"),
        ],
    );
    assert_eq!(missing_spec.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&missing_spec.stderr);
    assert!(stderr.contains("surface tui spec example.spec.mjs was not found in packages/tui"));
    assert!(stderr.ends_with("Your request was refused; nothing ran.\n"));
    assert!(!destination.path().join("apps/example").exists());
    assert!(!destination.path().join("apps/.adoptions.json").exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let symlink_source = source_repository();
        symlink(
            symlink_source
                .path()
                .join("packages/tui/tests/example.spec.mjs"),
            symlink_source
                .path()
                .join("packages/tui/tests/linked.spec.mjs"),
        )
        .expect("definition symlink");
        let symlink_refusal = run(
            destination.path(),
            &[
                "project",
                "adopt",
                "--source",
                symlink_source.path().to_str().expect("UTF-8 source"),
            ],
        );
        assert_eq!(symlink_refusal.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&symlink_refusal.stderr);
        assert!(stderr.contains("project definitions must not contain symlinks:"));
        assert!(stderr.ends_with("Your request was refused; nothing ran.\n"));
        assert!(!destination.path().join("apps/example").exists());
    }

    let missing_source = run(destination.path(), &["project", "adopt"]);
    assert_eq!(missing_source.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_source.stderr).contains("--source <repository>"));

    let extra_list_input = run(destination.path(), &["project", "adoptions", "extra"]);
    assert_eq!(extra_list_input.status.code(), Some(2));

    let invalid_index_file = destination.path().join("apps/.adoptions.json");
    fs::write(
        &invalid_index_file,
        b"{\"schema\":\"wrong\",\"sources\":[]}\n",
    )
    .expect("invalid adoption index");
    let invalid_index = run(destination.path(), &["project", "adoptions"]);
    assert_eq!(invalid_index.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invalid_index.stderr);
    assert!(stderr.contains("unsupported Probierz adoption index:"));
    assert!(stderr.ends_with("Your request was refused; nothing ran.\n"));

    let state = tempfile::tempdir().expect("onboarding state root");
    let replace_without_source = run_with_state(
        destination.path(),
        state.path(),
        &["onboarding", "--replace"],
    );
    assert_eq!(replace_without_source.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&replace_without_source.stderr).contains("--source <repository>")
    );
    assert!(!state.path().join("probierz/onboarding.json").exists());
}

#[test]
fn onboarding_writes_private_progress_and_supports_source_replace_reset_and_json() {
    let destination = destination_repository();
    let source = source_repository();
    let state = tempfile::tempdir().expect("onboarding state root");
    let state_file = state.path().join("probierz/onboarding.json");

    let initial = run_with_state(destination.path(), state.path(), &["onboarding", "--json"]);
    assert!(initial.status.success());
    let initial = json_output(&initial);
    assert_eq!(initial["product_id"], "probierz");
    assert_eq!(initial["journey_id"], "first-use");
    assert_eq!(initial["journey_version"], "2026-09-03.2");
    assert_eq!(
        initial["source_revision"],
        "probierz-first-use-2026-09-03.2"
    );
    assert_eq!(
        initial["first_success_fact"],
        "passing_quality_evidence_written"
    );
    assert_eq!(initial["status"], "in_progress");
    assert_eq!(initial["reset"], false);
    assert_eq!(initial["adoption"], Value::Null);
    assert_eq!(initial["screens"].as_array().map(Vec::len), Some(5));
    assert_eq!(initial["screens"][0]["screen_id"], "adopt-existing-project");
    assert_eq!(
        initial["screens"][4]["command"],
        "probierz run TARGET --app APP_ID --spec SPEC"
    );
    let progress: Value =
        serde_json::from_slice(&fs::read(&state_file).unwrap()).expect("onboarding progress");
    assert_eq!(progress["status"], "in_progress");
    assert_eq!(progress["evidence"], serde_json::json!({}));
    assert!(progress["started_at"]
        .as_str()
        .is_some_and(|value| value.ends_with('Z')));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&state_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let adopted = run_with_state(
        destination.path(),
        state.path(),
        &[
            "onboarding",
            "--source",
            source.path().to_str().expect("UTF-8 source"),
            "--json",
        ],
    );
    assert!(
        adopted.status.success(),
        "{}",
        String::from_utf8_lossy(&adopted.stderr)
    );
    let adopted = json_output(&adopted);
    assert_eq!(adopted["adoption"]["status"], "imported");
    assert_eq!(adopted["adoption"]["imported"], 3);
    assert_eq!(adopted["adoption"]["executedJourneys"], false);
    let progress: Value =
        serde_json::from_slice(&fs::read(&state_file).unwrap()).expect("adopted progress");
    assert_eq!(progress["evidence"]["project_definitions_adopted"], true);
    assert_eq!(
        progress["adoption"]["source_root"],
        adopted["adoption"]["sourceRoot"]
    );
    assert_eq!(
        progress["adoption"]["source_digest"],
        adopted["adoption"]["sourceDigest"]
    );
    assert!(progress["adoption"]["accepted_at"]
        .as_str()
        .is_some_and(|value| value.ends_with('Z')));

    let duplicate_with_replace = run_with_state(
        destination.path(),
        state.path(),
        &[
            "onboarding",
            "--source",
            source.path().to_str().expect("UTF-8 source"),
            "--replace",
            "--json",
        ],
    );
    assert!(duplicate_with_replace.status.success());
    let duplicate_with_replace = json_output(&duplicate_with_replace);
    assert_eq!(duplicate_with_replace["adoption"]["status"], "unchanged");
    assert_eq!(duplicate_with_replace["adoption"]["unchanged"], 3);

    let reset = run_with_state(
        destination.path(),
        state.path(),
        &["onboarding", "--reset", "--json"],
    );
    assert!(reset.status.success());
    let reset = json_output(&reset);
    assert_eq!(reset["reset"], true);
    assert_eq!(reset["adoption"], Value::Null);
    let progress: Value =
        serde_json::from_slice(&fs::read(&state_file).unwrap()).expect("reset progress");
    assert_eq!(progress["status"], "in_progress");
    assert_eq!(progress["evidence"], serde_json::json!({}));
    assert!(progress.get("adoption").is_none());

    let rendered = run_with_state(destination.path(), state.path(), &["onboarding"]);
    assert!(rendered.status.success());
    let stdout = String::from_utf8_lossy(&rendered.stdout);
    assert!(stdout.starts_with("1/5  Bring your existing Probierz project\n"));
    assert!(stdout.ends_with("No passing quality evidence written from this shell yet, so passing_quality_evidence_written is still open; the next passing completed run closes it.\n"));
}

#[test]
fn onboarding_conflict_uses_the_documented_sentence_and_replace_accepts_it() {
    let destination = destination_repository();
    let source = source_repository();
    let state = tempfile::tempdir().expect("onboarding state root");
    fs::create_dir_all(destination.path().join("apps/example")).expect("destination app");
    let destination_manifest = destination.path().join("apps/example/probierz.yaml");
    fs::write(&destination_manifest, "unmanaged\n").expect("unmanaged destination manifest");

    let refused = run_with_state(
        destination.path(),
        state.path(),
        &[
            "onboarding",
            "--source",
            source.path().to_str().expect("UTF-8 source"),
        ],
    );
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&refused.stdout);
    assert!(stdout.starts_with(
        "Existing project not adopted: 1 conflicting definition(s). No files changed.\n       apps/example/probierz.yaml: destination content or mode differs; repeat with explicit replacement\n       Resolve the files or repeat with --replace after reviewing the conflicts.\n"
    ));
    assert_eq!(
        fs::read_to_string(&destination_manifest).unwrap(),
        "unmanaged\n"
    );
    assert!(!destination.path().join("apps/.adoptions.json").exists());

    let accepted = run_with_state(
        destination.path(),
        state.path(),
        &[
            "onboarding",
            "--source",
            source.path().to_str().expect("UTF-8 source"),
            "--replace",
        ],
    );
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let stdout = String::from_utf8_lossy(&accepted.stdout);
    assert!(stdout.starts_with(
        "Existing project imported: 3 imported, 0 unchanged, 0 removed.\n       Journey definitions were persisted but not run.\n"
    ));
    assert!(destination.path().join("apps/.adoptions.json").is_file());
}
