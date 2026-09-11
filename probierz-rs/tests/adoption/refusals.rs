//! The documented arguments, and the selections and shapes refused before any mutation.

use crate::*;

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
    assert!(adopt.contains("Usage: probierz project adopt --source <repository> [--replace]"));
    assert!(adopt.contains("--source <repository>"));
    assert!(adopt.contains("--replace"));

    let adoptions = run(root.path(), &["project", "adoptions", "--help"]);
    assert!(adoptions.status.success());
    assert!(
        String::from_utf8_lossy(&adoptions.stdout).contains("Usage: probierz project adoptions")
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
