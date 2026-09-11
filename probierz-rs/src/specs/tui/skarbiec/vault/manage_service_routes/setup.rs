//! The journey's setup: the scratch vault and its paths, the fixture shell, the
//! routes help, the owner's vault, and the login item every route will point at.

use std::path::Path;
use std::time::Duration;

use serde_json::json;

use crate::specs;
use crate::specs::tui::skarbiec::fixture::{self as fixture, Shell};

use super::journey::{ok_json, Journey, LOGIN_ITEM, SECRET_VALUE};

/// Starts the journey and returns it with the length of the shell log at the
/// point the routes commands begin, so the redaction check reads only them.
pub(super) fn start<'a>(
    context: &'a specs::Context,
    binary: &'a str,
    temp_dir: &'a Path,
) -> Result<(Journey<'a>, usize), String> {
    let vault_file = temp_dir.join("routes.vault.json");
    let routes_table = temp_dir.join("capability-routes.json");
    let beside_journal = temp_dir.join("capability-routes.audit.jsonl");
    let routes_audit_file = temp_dir.join("routes.audit.jsonl");
    let setup_audit_file = temp_dir.join("fixture-setup.audit.jsonl");
    let env = fixture::env(&[
        ("GNUPGHOME", temp_dir),
        ("SKARBIEC_VAULT_FILE", &vault_file),
        ("SKARBIEC_AUDIT_FILE", &routes_audit_file),
        ("SKARBIEC_CAPABILITY_ROUTES_FILE", &routes_table),
    ]);
    let shell = Shell::spawn(
        "__SKARBIEC_ROUTES_READY__",
        "__SKARBIEC_ROUTES_",
        None,
        &env,
        120,
        36,
    )?;
    let executed = Vec::new();
    let mut journey = Journey {
        context,
        binary,
        temp_dir,
        routes_table,
        beside_journal,
        routes_audit_file,
        shell,
        executed,
    };

    let help = ok_json(
        &mut journey.shell,
        journey.binary,
        &["routes", "help"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes help",
        &mut journey.executed,
    )?;
    fixture::ensure(
        help["table"].as_str() == Some(journey.routes_table.to_string_lossy().as_ref()),
        format!("routes help resolved the wrong table: {help}"),
    )?;
    fixture::ensure(
        help["commands"]
            == json!([
                "routes list [<consumer>]",
                "routes add --resource <resource> --item <item> --field <field> --reason <text>",
                "routes verify [<consumer>]"
            ]),
        format!("routes help returned the wrong commands: {help}"),
    )?;
    let setup_path = setup_audit_file.to_string_lossy().into_owned();
    let initialized = ok_json(
        &mut journey.shell,
        journey.binary,
        &["init", "routes-journey-owner"],
        &[("SKARBIEC_AUDIT_FILE", setup_path.as_str())],
        Duration::from_secs(180),
        "skarbiec init routes-journey-owner",
        &mut journey.executed,
    )?;
    fixture::ensure(
        initialized["ok"] == true,
        "vault initialization did not report ok",
    )?;
    fixture::ensure(
        initialized["vault"].as_str() == Some(vault_file.to_string_lossy().as_ref()),
        format!("initialized vault path is not {}", vault_file.display()),
    )?;
    let secret_field = format!("password={SECRET_VALUE}");
    let stored = ok_json(
        &mut journey.shell,
        journey.binary,
        &[
            "set",
            LOGIN_ITEM,
            "--type",
            "login",
            "username=ops@cloudflare.invalid",
            &secret_field,
        ],
        &[("SKARBIEC_AUDIT_FILE", setup_path.as_str())],
        Duration::from_secs(60),
        &format!("skarbiec set {LOGIN_ITEM} (redacted)"),
        &mut journey.executed,
    )?;
    fixture::ensure(
        stored == json!({"id":LOGIN_ITEM,"kind":"login","ok":true}),
        format!("stored answer is wrong: {stored}"),
    )?;
    let routes_phase_start = journey.shell.full_log().len();
    Ok((journey, routes_phase_start))
}
