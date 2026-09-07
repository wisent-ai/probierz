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

fn source_with_identical_definitions(source: &Path) -> TempDir {
    let duplicate = tempfile::tempdir().expect("second source repository");
    repository(duplicate.path());
    for relative in [
        "apps/example/probierz.yaml",
        "packages/tui/tests/example.spec.mjs",
        "packages/tui/tests/support.mjs",
    ] {
        let target = duplicate.path().join(relative);
        fs::create_dir_all(target.parent().expect("definition parent"))
            .expect("second source definition directory");
        fs::copy(source.join(relative), &target).expect("copy identical source definition");
        #[cfg(unix)]
        {
            fs::set_permissions(&target, fs::metadata(source.join(relative)).unwrap().permissions())
                .expect("copy source definition mode");
        }
    }
    duplicate
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

fn assert_invocation_refused(root: &Path, arguments: &[&str], sentence: &str) {
    let output = run(root, arguments);
    assert_eq!(output.status.code(), Some(2), "{arguments:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(sentence),
        "{arguments:?} did not report {sentence:?}:\n{stderr}"
    );
}

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
            fs::metadata(
                destination
                    .path()
                    .join("packages/tui/tests/support.mjs")
            )
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
        &[
            "project",
            "adopt",
            "--source",
            second_text,
            "--replace",
        ],
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

#[test]
fn documented_help_names_each_adoption_argument() {
    let root = destination_repository();
    let onboarding = run(root.path(), &["onboarding", "--help"]);
    assert!(onboarding.status.success());
    let onboarding = String::from_utf8_lossy(&onboarding.stdout);
    for value in ["--reset", "--source <repository>", "--replace", "--json"] {
        assert!(onboarding.contains(value), "missing {value}:\n{onboarding}");
    }

    let project = run(root.path(), &["project", "--help"]);
    assert!(project.status.success());
    let project = String::from_utf8_lossy(&project.stdout);
    assert!(project.contains("Usage: probierz project [OPTIONS] <COMMAND>"));
    assert!(project.contains("adopt"));
    assert!(project.contains("adoptions"));

    let adopt = run(root.path(), &["project", "adopt", "--help"]);
    assert!(adopt.status.success());
    let adopt = String::from_utf8_lossy(&adopt.stdout);
    assert!(adopt.contains(
        "Usage: probierz project adopt --source <repository> [--replace]"
    ));
    assert!(adopt.contains("--source <repository>"));
    assert!(adopt.contains("--replace"));

    let adoptions = run(root.path(), &["project", "adoptions", "--help"]);
    assert!(adoptions.status.success());
    assert!(String::from_utf8_lossy(&adoptions.stdout)
        .contains("Usage: probierz project adoptions"));
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

    assert_invocation_refused(
        destination.path(),
        &["project", "adopt"],
        "project adopt needs --source <repository>",
    );
    assert_invocation_refused(
        destination.path(),
        &["project", "adoptions", "extra"],
        "project adoptions accepts no options",
    );
    assert_invocation_refused(
        destination.path(),
        &["project"],
        "Usage: probierz project [OPTIONS] <COMMAND>",
    );
    assert_invocation_refused(
        destination.path(),
        &["project", "unknown"],
        "Usage: probierz project [OPTIONS] <COMMAND>",
    );
    assert_invocation_refused(
        destination.path(),
        &["project", "adopt", "--source"],
        "a value is required for '--source <repository>'",
    );
    assert_invocation_refused(
        destination.path(),
        &["project", "adopt", "--unknown"],
        "unknown project adoption option: --unknown",
    );
    assert_invocation_refused(
        destination.path(),
        &["project", "adopt", "unexpected"],
        "unexpected project adoption argument: unexpected",
    );
    let valid_source = source_repository();
    let valid_source = valid_source.path().to_str().expect("UTF-8 valid source");
    assert_invocation_refused(
        destination.path(),
        &[
            "project",
            "adopt",
            "--source",
            valid_source,
            "--source",
            valid_source,
        ],
        "cannot be used multiple times",
    );

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
    for (arguments, sentence) in [
        (
            vec!["onboarding", "--replace"],
            "--replace requires --source <repository>",
        ),
        (
            vec!["onboarding", "--source"],
            "--source needs a repository path",
        ),
        (
            vec!["onboarding", "--unknown"],
            "unknown onboarding option: --unknown",
        ),
        (
            vec!["onboarding", "unexpected"],
            "unknown onboarding option: unexpected",
        ),
        (
            vec!["onboarding", "--reset", "--reset"],
            "--reset may be supplied only once",
        ),
    ] {
        let output = run_with_state(destination.path(), state.path(), &arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(sentence),
            "{arguments:?}"
        );
    }
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
    assert_eq!(
        initial["screens"],
        serde_json::json!([
            {
                "screen_id": "adopt-existing-project",
                "title": "Bring your existing Probierz project",
                "body": "Choose another Probierz repository to adopt its validated apps/<appId>/probierz.yaml manifests and established package spec directories. Probierz preserves the definitions, reports every conflict, and does not run a journey. Skip keeps this project empty and usable.",
                "command": null
            },
            {
                "screen_id": "choose-one-journey",
                "title": "Start with one declared journey",
                "body": "Probierz runs evidence for a product, surface and user journey declared in an application manifest. Begin with `probierz apps`, then inspect one registration with `probierz app APP_ID`; its surface names the target and spec you can run instead of guessing either.",
                "command": null
            },
            {
                "screen_id": "read-the-evidence",
                "title": "A completed run leaves quality evidence",
                "body": "The first durable result is a run manifest, not a claim that a suite passed. Probierz binds the report, analysis, source and build identities, conditions and artifact hashes into that record, then reports a pass or fail without averaging failures away.",
                "command": null
            },
            {
                "screen_id": "receipts-follow-runs",
                "title": "Release receipts come after recorded runs",
                "body": "A release gate consumes exact run IDs and identities. Once the required journeys have qualifying evidence, `probierz receipt` signs the resulting verdict for a release; it cannot replace the underlying run records or turn missing evidence green.",
                "command": null
            },
            {
                "screen_id": "produce-evidence",
                "title": "Produce your first evidence record",
                "body": "Run one declared spec on its target with `probierz run TARGET --app APP_ID --spec SPEC`. When the command succeeds and Probierz writes a passing evidence block into the run manifest, this journey is complete. A failed run remains honest, actionable evidence, but the release gate stays red and this first-success step stays open.",
                "command": "probierz run TARGET --app APP_ID --spec SPEC"
            }
        ])
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
