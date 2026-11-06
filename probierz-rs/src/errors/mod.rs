//! The ways a Probierz command refuses that are declared, each once as a
//! cause of a `wisent_errors::Operation` naming what failed, why, the state
//! the refusal shows, the fix and the real test that provokes it. Probierz
//! declares the causes its tests provoke; a failure point not listed is not
//! declared yet. `probierz refusals list|show|check` prints the
//! declarations and holds them to the standard: a cause without its test is
//! refused.

pub mod cli;

use std::path::Path;

pub use wisent_errors::{Cause, Code as Kind, Operation};

/// The documentation page every cause is a line on: the page of the command
/// that lists them.
const DOCS: &str = "https://probierz.wisent.com/docs/cli/refusals/list";

/// The cause a declaration starts from: a refusal of the request.
const PLAIN: Cause = Cause {
    id: "",
    code: Kind::Refused,
    usage: false,
    exit: None,
    failed: "",
    cause: "",
    observed: "",
    remedy: "",
    test: "",
};

pub static HARNESS_UNRESOLVED: Cause = Cause {
    id: "harness-unresolved",
    code: Kind::Config,
    failed: "finding the harness every command but intake reads",
    cause: "neither --harness, PROBIERZ_HARNESS_DIR, the build's own checkout nor the working directory holds an apps/ directory",
    observed: "the refusal names every directory it looked in",
    remedy: "pass --harness with the checkout that holds apps/, or run from it",
    test: "tests/errors/main.rs::a_harness_without_apps_is_refused_naming_where_it_looked",
    ..PLAIN
};

pub static INPUT_MISSING: Cause = Cause {
    id: "input-missing",
    failed: "reading a file or directory the command was given",
    cause: "the path names nothing on this machine",
    observed: "the refusal names the path",
    remedy: "give a path that exists",
    test: "tests/inspect/names.rs::gif_refuses_a_missing_video_and_writes_nothing",
    ..PLAIN
};

pub static INCIDENT_CLAIM_EMPTY: Cause = Cause {
    id: "incident-claim-empty",
    failed: "recording an incident",
    cause: "--claim is blank, so the register would hold an attempt that claims nothing",
    observed: "the refusal names --claim; nothing is written to the register",
    remedy: "say with --claim what the attempt claimed",
    test: "tests/errors/main.rs::an_incident_without_a_claim_is_refused_and_nothing_is_written",
    ..PLAIN
};

pub static INCIDENT_REGISTER_DAMAGED: Cause = Cause {
    id: "incident-register-damaged",
    code: Kind::Config,
    failed: "reading the incident register",
    cause: "a line resolves an incident no earlier line records, or resolves one already resolved",
    observed: "the refusal names the register file and the line",
    remedy: "restore the register from its backup, or remove the line the refusal names",
    test: "tests/incidents/register.rs::a_resolution_of_an_incident_the_register_does_not_hold_is_refused_naming_its_line",
    ..PLAIN
};

pub static GATE_HOOK_FOREIGN: Cause = Cause {
    id: "gate-hook-foreign",
    code: Kind::Config,
    failed: "removing the pre-push hook",
    cause: "the hook in place is not the one `probierz gate install` wrote",
    observed: "the refusal names the hook; it is left as it was",
    remedy: "remove or rename the hook yourself, or keep it",
    test: "tests/gate/group.rs::gate_uninstall_refuses_a_hook_it_did_not_write",
    ..PLAIN
};

pub static SECRET_FOUND: Cause = Cause {
    id: "secret-found",
    failed: "scanning evidence for credentials",
    cause: "a file holds a credential or a key block",
    observed: "the report names each file and line, never the value",
    remedy: "remove the credential from the evidence and rotate it",
    test: "tests/secrets/group.rs::a_private_key_block_is_refused_and_named",
    ..PLAIN
};

pub static INTAKE_TOKEN_EMPTY: Cause = Cause {
    id: "intake-token-empty",
    code: Kind::Config,
    failed: "starting the failure intake",
    cause: "PROBIERZ_INTAKE_TOKEN is set but empty",
    observed: "the refusal names the variable; nothing listens",
    remedy: "set it to the token clients send, or unset it to use the stored one",
    test: "tests/intake/serve.rs::an_empty_environment_token_is_refused_by_name",
    ..PLAIN
};

pub static ROUTER_UNAVAILABLE: Cause = Cause {
    id: "router-unavailable",
    code: Kind::InfraDown,
    failed: "asking a model for a repair, a draft or a grade",
    cause: "no model router is named and no inference alternative of Stado answered",
    observed: "the refusal names each alternative it asked and what it answered; the request is kept for an attestation",
    remedy: "name the router in STADO_MODEL_ROUTER_URL, or answer the kept request with `stado capabilities attestation record inference --consumer probierz`",
    test: "tests/repair/model_service.rs::a_repair_with_no_router_named_and_no_alternative_answering_is_refused_naming_each",
    ..PLAIN
};

pub static SUITE_FAILED: Cause = Cause {
    id: "suite-failed",
    failed: "running a checkout's own suites",
    cause: "a gate the release manifest declares failed or its report could not be collected",
    observed: "the run record names each gate, its exit status and the report it refused",
    remedy: "read the gate's output the record keeps, and fix the gate or its report",
    test: "tests/suites/run.rs::a_symlink_report_refuses_collection_but_keeps_the_run",
    ..PLAIN
};

pub static PROBIERZ: Operation = Operation {
    id: "probierz",
    command: "probierz",
    summary: "Runs journeys where the product lives and keeps every run as evidence; a refusal names the step, the state and the fix.",
    docs: DOCS,
    causes: &[
        &HARNESS_UNRESOLVED,
        &INPUT_MISSING,
        &INCIDENT_CLAIM_EMPTY,
        &INCIDENT_REGISTER_DAMAGED,
        &GATE_HOOK_FOREIGN,
        &SECRET_FOUND,
        &INTAKE_TOKEN_EMPTY,
        &ROUTER_UNAVAILABLE,
        &SUITE_FAILED,
    ],
};

pub static OPERATIONS: &[&Operation] = &[&PROBIERZ];

/// Everything that keeps the catalogue from meeting the standard: each
/// declaration's own problems, and each cause whose test file under `root`
/// does not declare the test function it names.
pub fn problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for operation in OPERATIONS {
        problems.extend(operation.problems());
        for cause in operation.causes {
            let Some((file, name)) = cause.test.split_once("::") else {
                continue;
            };
            let path = root.join(file);
            match std::fs::read_to_string(&path) {
                Ok(text) if text.contains(&format!("fn {name}(")) => {}
                Ok(_) => problems.push(format!(
                    "cause {}: {} declares no test function `{name}`",
                    cause.id,
                    path.display()
                )),
                Err(error) => problems.push(format!(
                    "cause {}: its test file {} cannot be read: {error}",
                    cause.id,
                    path.display()
                )),
            }
        }
    }
    problems
}
