use std::collections::BTreeMap;
use std::time::Duration;

use regex::Regex;
use serde_json::json;

use crate::{specs, tui};

use super::skarbiec_fixture::{self as fixture};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::required_binary(context)?;
    let temp_dir = fixture::scratch("probierz-skarbiec-first-use")?;
    let vault_file = temp_dir.join("onboarding.vault.json");
    let audit_file = temp_dir.join("audit.jsonl");
    let env = BTreeMap::from([
        ("HOME".to_string(), temp_dir.to_string_lossy().into_owned()),
        (
            "GNUPGHOME".to_string(),
            temp_dir.to_string_lossy().into_owned(),
        ),
        (
            "SKARBIEC_VAULT_FILE".to_string(),
            vault_file.to_string_lossy().into_owned(),
        ),
        (
            "SKARBIEC_AUDIT_FILE".to_string(),
            audit_file.to_string_lossy().into_owned(),
        ),
        (
            "USER".to_string(),
            "probierz-skarbiec-first-use".to_string(),
        ),
        ("HOSTNAME".to_string(), "probierz-isolated-host".to_string()),
    ]);
    let result = run_isolated(context, &binary, &env);
    fixture::clean(&temp_dir);
    result
}

fn run_isolated(
    context: &specs::Context,
    binary: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let initialized = run_once(
        binary,
        &["init", "probierz-skarbiec-onboarding-owner"],
        env,
        "owner_fpr",
        Duration::from_secs(120),
    )?;
    let ok_pattern = Regex::new(r#""ok"\s*:\s*true"#).map_err(|error| error.to_string())?;
    let completed_pattern =
        Regex::new(r#""status"\s*:\s*"completed""#).map_err(|error| error.to_string())?;
    fixture::ensure(
        ok_pattern.is_match(&initialized),
        format!("initialization did not report ok: {initialized}"),
    )?;
    fixture::ensure(
        !completed_pattern.is_match(&initialized),
        "initialization incorrectly reported onboarding completed",
    )?;

    let mut onboarding = spawn(binary, &["onboarding"], env, 120, 40)?;
    onboarding
        .wait_for(
            "Your agents never hold a credential",
            Duration::from_secs(30),
            true,
        )
        .map_err(|error| error.to_string())?;
    onboarding.key("enter").map_err(|error| error.to_string())?;
    onboarding
        .wait_for(
            "From .env copies to one-use capabilities",
            Duration::from_secs(15),
            true,
        )
        .map_err(|error| error.to_string())?;
    onboarding.key("enter").map_err(|error| error.to_string())?;
    onboarding
        .wait_for(
            "Create and read a safe local note",
            Duration::from_secs(15),
            true,
        )
        .map_err(|error| error.to_string())?;
    onboarding.send("n").map_err(|error| error.to_string())?;
    onboarding.key("enter").map_err(|error| error.to_string())?;
    let paused = onboarding
        .wait_for("paused", Duration::from_secs(15), true)
        .map_err(|error| error.to_string())?;
    let paused_pattern =
        Regex::new(r#""status"\s*:\s*"paused""#).map_err(|error| error.to_string())?;
    let resume_pattern =
        Regex::new(r#""resume"\s*:\s*"skarbiec onboarding""#).map_err(|error| error.to_string())?;
    fixture::ensure(
        paused_pattern.is_match(&paused),
        format!("onboarding did not report paused: {paused}"),
    )?;
    fixture::ensure(
        resume_pattern.is_match(&paused),
        format!("paused onboarding did not report its resume command: {paused}"),
    )?;
    fixture::ensure(
        !paused.contains("Created and decrypted non-secret note"),
        "paused onboarding created the demo note",
    )?;
    onboarding.close().map_err(|error| error.to_string())?;

    let completing = spawn(binary, &["onboarding", "--yes"], env, 120, 40)?;
    let completed = completing
        .wait_for("first_success", Duration::from_secs(120), true)
        .map_err(|error| error.to_string())?;
    let created_pattern =
        Regex::new(r"Created and decrypted non-secret note: onboarding-safe-note-[0-9a-f]{8}")
            .map_err(|error| error.to_string())?;
    let observed_pattern =
        Regex::new(r"Observed hash-chained audit entry for item: onboarding-safe-note-[0-9a-f]{8}")
            .map_err(|error| error.to_string())?;
    fixture::ensure(
        created_pattern.is_match(&completed),
        format!("completion did not report its created note: {completed}"),
    )?;
    fixture::ensure(
        observed_pattern.is_match(&completed),
        format!("completion did not report its audit observation: {completed}"),
    )?;
    fixture::ensure(
        completed.contains("The note value is not present in the audit record."),
        "completion did not say the note value was absent from the audit record",
    )?;
    fixture::ensure(
        completed_pattern.is_match(&completed),
        format!("onboarding did not report completed: {completed}"),
    )?;
    let item_pattern =
        Regex::new(r"onboarding-safe-note-[0-9a-f]{8}").map_err(|error| error.to_string())?;
    let item_id = item_pattern
        .find(&completed)
        .map(|value| value.as_str().to_string())
        .ok_or_else(|| {
            "expected the isolated demo item id in canonical completion output".to_string()
        })?;
    completing.close().map_err(|error| error.to_string())?;

    let audit = run_once(
        binary,
        &[
            "audit-query",
            "--op",
            "onboarding-demo-item-read",
            "--item",
            &item_id,
        ],
        env,
        "matched",
        Duration::from_secs(15),
    )?;
    let matched_pattern =
        Regex::new(r#""matched"\s*:\s*[1-9]\d*"#).map_err(|error| error.to_string())?;
    let operation_pattern = Regex::new(r#""op"\s*:\s*"onboarding-demo-item-read""#)
        .map_err(|error| error.to_string())?;
    let item_field_pattern = Regex::new(&format!(r#""item"\s*:\s*"{}""#, regex::escape(&item_id)))
        .map_err(|error| error.to_string())?;
    fixture::ensure(
        matched_pattern.is_match(&audit),
        format!("audit query reported no match: {audit}"),
    )?;
    fixture::ensure(
        operation_pattern.is_match(&audit),
        format!("audit query omitted onboarding-demo-item-read: {audit}"),
    )?;
    fixture::ensure(
        item_field_pattern.is_match(&audit),
        format!("audit query omitted item {item_id}: {audit}"),
    )?;
    fixture::ensure(
        !audit.contains("Skarbiec onboarding note; explicitly not a secret"),
        "audit query exposed the note value",
    )?;

    fixture::write_trace(
        context,
        "skarbiec-onboarding-first-use.trace.json",
        json!({
            "schemaVersion": 1,
            "kind": "probierz-skarbiec-onboarding-trace",
            "evidenceLevel": "E2",
            "runId": context.optional("PROBIERZ_RUN_ID"),
            "status": "completed",
            "observation": {
                "firstSuccess": "audit_entry_observed",
                "itemId": item_id,
                "auditOperation": "onboarding-demo-item-read",
            },
            "redaction": {
                "status": "verified_redacted",
                "credentialsIncluded": false,
                "itemValuesIncluded": false,
            },
            "publicationRequirements": {
                "artifactKind": "trace",
                "minimumEvidence": "E2",
                "redactionStatus": "verified_redacted",
                "signedReceiptRequired": true,
            }
        }),
    )?;
    Ok(())
}

fn spawn(
    binary: &str,
    args: &[&str],
    env: &BTreeMap<String, String>,
    cols: u16,
    rows: u16,
) -> Result<tui::Terminal, String> {
    let mut request = tui::Spawn::new(binary)
        .args(args.iter().copied())
        .size(cols, rows);
    for (name, value) in env {
        request = request.env(name, value);
    }
    tui::Terminal::spawn(request).map_err(|error| error.to_string())
}

fn run_once(
    binary: &str,
    args: &[&str],
    env: &BTreeMap<String, String>,
    marker: &str,
    timeout: Duration,
) -> Result<String, String> {
    let terminal = spawn(binary, args, env, 120, 36)?;
    let log = terminal
        .wait_for(marker, timeout, true)
        .map_err(|error| error.to_string())?;
    terminal.close().map_err(|error| error.to_string())?;
    Ok(log)
}
