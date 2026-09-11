//! The documented refusals: required inputs, identity and cancellation, and the run options.

use crate::*;

#[test]
fn documented_required_input_refusals_are_exact() {
    let root = harness();
    refused(
        root.path(),
        &["stado", "run"],
        "stado run needs a target (e.g. tui)",
    );
    refused(
        root.path(),
        &["stado", "run", "tui"],
        "stado run needs --app <appId>",
    );
    refused(
        root.path(),
        &["stado", "author"],
        "stado author needs an app ID and a journey name",
    );
    refused(
        root.path(),
        &["stado", "author", "demo", "journey"],
        "stado author needs --target <t>",
    );
    refused(
        root.path(),
        &["stado", "author", "demo", "journey", "--target", "web"],
        "stado author needs --desc <journey goal>",
    );
    refused(root.path(), &["stado", "seo"], "stado seo needs an app ID");
    refused(
        root.path(),
        &["stado", "collect", "job-0123456789abcdef01234567"],
        "stado collect needs --app <appId>",
    );
}

#[test]
fn documented_identity_and_cancellation_refusals_are_exact() {
    let root = harness();
    refused(
        root.path(),
        &["stado", "collect", "not-a-job", "--app", "demo"],
        "Collection requires a canonical Stado job ID and a known Stado host.",
    );
    refused(
        root.path(),
        &["stado", "resume", "../job"],
        "Resuming remote evidence needs a valid existing Stado job ID.",
    );
    refused(
        root.path(),
        &["stado", "cancel", "job-0123456789abcdef01234567"],
        "stado cancel needs --host <host>",
    );
    refused(
        root.path(),
        &[
            "stado",
            "cancel",
            "job-0123456789abcdef01234567",
            "--host",
            "stado:any",
        ],
        "stado cancel needs --reason <reason>",
    );
}

#[test]
fn documented_run_option_refusals_are_exact() {
    let root = harness();
    refused(
        root.path(),
        &[
            "stado",
            "run",
            "tui",
            "--app",
            "demo",
            "--env",
            "9BAD=value",
        ],
        "--env needs NAME=VALUE with a valid environment variable name",
    );
    refused(
        root.path(),
        &[
            "stado",
            "run",
            "tui",
            "--app",
            "demo",
            "--script",
            "remote/run.sh",
        ],
        "--script requires --node-source (custom app jobs run from app sources)",
    );
    refused(
        root.path(),
        &["stado", "run", "tui", "--app", "demo", "--app-binary-path"],
        "--app-binary-path needs a value",
    );
    refused(
        root.path(),
        &[
            "stado",
            "run",
            "tui",
            "--app",
            "demo",
            "--app-binary-path",
            "/tmp/demo",
        ],
        "--app-binary-path requires --app-repo <path>",
    );
    refused(
        root.path(),
        &[
            "stado", "run", "tui", "--app", "demo", "--cargo-release", "--node-source",
        ],
        "remote application provisioning options are mutually exclusive: --cargo-release, --node-source",
    );
    refused(
        root.path(),
        &["stado", "run", "tui", "--app", "demo", "--binary", "demo"],
        "--binary and --cargo-manifest require --cargo-release",
    );
}
