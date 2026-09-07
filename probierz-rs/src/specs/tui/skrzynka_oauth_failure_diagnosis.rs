use crate::specs::{self, tui::common};
use std::{path::PathBuf, process::Command};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let repo = PathBuf::from(context.optional("SKRZYNKA_REPO").unwrap_or_else(|| {
        "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skrzynka".into()
    }));
    if !repo.join("Cargo.toml").exists() {
        return Err(format!(
            "no skrzynka checkout at {}; set SKRZYNKA_REPO",
            repo.display()
        ));
    }
    let output = Command::new("cargo")
        .args(["test", "--locked", "--bins", "--", "--nocapture"])
        .current_dir(&repo)
        .output()
        .map_err(|error| format!("cannot start cargo: {error}"))?;
    let run = common::Output {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    let output = run.combined();
    if !run.status.success() {
        return Err(format!(
            "cargo test --bins failed with {}\n{output}",
            run.code()
                .map_or_else(|| "signal".into(), |c| c.to_string())
        ));
    }
    let required = [
        "gmail::tests::the_captured_google_error_decodes_to_its_code",
        "gmail::tests::a_landing_url_without_an_error_carries_no_code",
        "gmail::tests::the_operands_come_from_the_url_that_was_handed_out",
        "gmail::tests::the_refusal_names_the_client_the_uri_and_the_setting",
        "gmail::tests::google_imap_password_rejected_names_mailbox_and_credential_item",
        "gmail::tests::google_imap_password_rejected_enforces_gmail_host_boundary",
    ];
    for name in required {
        let line = format!("test {name} ... ok");
        if !output.contains(&line) {
            return Err(format!("{name} did not run and pass; a renamed or removed test is not a passing one\n{output}"));
        }
    }
    let summary = format!("test result: ok. {} passed; 0 failed;", required.len());
    if !output.contains(&summary) {
        return Err(format!(
            "expected exactly {} passing unit tests and no failures\n{output}",
            required.len()
        ));
    }
    Ok(())
}
