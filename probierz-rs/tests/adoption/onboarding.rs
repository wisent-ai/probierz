//! Onboarding writes private progress and supports source replace, reset and JSON.

use crate::*;

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
