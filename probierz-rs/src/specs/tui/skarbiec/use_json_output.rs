use std::time::Duration;

use regex::Regex;
use serde_json::json;

use crate::specs;

use super::fixture::{self as fixture, Shell};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::binary(context);
    let temp_dir = fixture::scratch("skarbiec-json-output")?;
    let vault_file = temp_dir.join("json-output.vault.json");
    let audit_file = temp_dir.join("json-output.audit.jsonl");
    let result = (|| {
        let env = fixture::env(&[
            ("GNUPGHOME", &temp_dir),
            ("SKARBIEC_VAULT_FILE", &vault_file),
            ("SKARBIEC_AUDIT_FILE", &audit_file),
        ]);
        let mut shell = Shell::spawn(
            "__SKARBIEC_JSON_OUTPUT_READY__",
            "__SKARBIEC_JSON_OUTPUT_",
            None,
            &env,
            120,
            36,
        )?;
        let run_json = |shell: &mut Shell,
                        args: &[&str],
                        timeout: Duration|
         -> Result<serde_json::Value, String> {
            let description = args.join(" ");
            let result = shell.run_program(&binary, args, &[], timeout)?;
            fixture::ensure(
                result.status == 0,
                format!("skarbiec {description} exited unsuccessfully"),
            )?;
            let parsed = fixture::parse_json(&result.output, || {
                format!("skarbiec {description} did not emit JSON")
            })?;
            fixture::ensure(
                parsed.is_object() || parsed.is_array(),
                format!("skarbiec {description} emitted a non-structured JSON value"),
            )?;
            Ok(parsed)
        };
        let menu = run_json(&mut shell, &[], Duration::from_secs(30))?;
        for command in [
            "init", "set", "get", "list", "delete", "restore", "purge", "generate",
        ] {
            fixture::ensure(
                fixture::strings(&menu, "/commands").contains(&command),
                format!("command menu is missing {command}"),
            )?;
        }
        let initialized = run_json(
            &mut shell,
            &["init", "json-output-e2e-owner"],
            Duration::from_secs(120),
        )?;
        fixture::ensure(
            initialized["ok"] == true,
            "vault initialization did not report ok",
        )?;
        fixture::ensure(
            initialized["vault"].as_str() == Some(vault_file.to_string_lossy().as_ref()),
            format!("initialized vault path is not {}", vault_file.display()),
        )?;
        let fingerprint = Regex::new(r"^[0-9A-F]+$").map_err(|error| error.to_string())?;
        fixture::ensure(
            fingerprint.is_match(initialized["owner_fpr"].as_str().unwrap_or_default()),
            "owner fingerprint is not uppercase hexadecimal",
        )?;
        fixture::ensure(
            fingerprint.is_match(initialized["recovery_fpr"].as_str().unwrap_or_default()),
            "recovery fingerprint is not uppercase hexadecimal",
        )?;
        let id = "machine-readable-note";
        let value = "json-secret-73c5d9";
        let field = format!("value={value}");
        let stored = run_json(
            &mut shell,
            &["set", id, "--type", "note", &field],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            stored == json!({"id":id,"kind":"note","ok":true}),
            format!("stored answer is wrong: {stored}"),
        )?;
        let retrieved = run_json(&mut shell, &["get", id], Duration::from_secs(30))?;
        let item = json!({"schema":"skarbiec.item.v2","kind":"note","fields":{"value":value},"context":{}});
        fixture::ensure(
            retrieved == item,
            format!("retrieved answer is wrong: {retrieved}"),
        )?;
        let listed = run_json(&mut shell, &["list"], Duration::from_secs(30))?;
        fixture::ensure(
            listed.as_array().map(Vec::len) == Some(1),
            format!("list is wrong: {listed}"),
        )?;
        fixture::ensure(
            listed[0]["id"] == id && listed[0]["kind"] == "note" && listed[0]["deleted"] == false,
            format!("listed row is wrong: {}", listed[0]),
        )?;
        fixture::ensure(
            run_json(&mut shell, &["delete", id], Duration::from_secs(30))? == json!({"ok":true}),
            "delete did not report ok",
        )?;
        fixture::ensure(
            run_json(&mut shell, &["list"], Duration::from_secs(30))? == json!([]),
            "live list after delete is not empty",
        )?;
        let trashed = run_json(&mut shell, &["list", "--all"], Duration::from_secs(30))?;
        fixture::ensure(
            trashed.as_array().map(Vec::len) == Some(1)
                && trashed[0]["id"] == id
                && trashed[0]["deleted"] == true,
            format!("trash is wrong: {trashed}"),
        )?;
        fixture::ensure(
            run_json(&mut shell, &["restore", id], Duration::from_secs(30))? == json!({"ok":true}),
            "restore did not report ok",
        )?;
        fixture::ensure(
            run_json(&mut shell, &["get", id], Duration::from_secs(30))? == item,
            "restored item does not decrypt to its original value",
        )?;
        fixture::ensure(
            run_json(&mut shell, &["delete", id], Duration::from_secs(30))? == json!({"ok":true}),
            "second delete did not report ok",
        )?;
        fixture::ensure(
            run_json(&mut shell, &["purge", id], Duration::from_secs(30))? == json!({"ok":true}),
            "purge did not report ok",
        )?;
        fixture::ensure(
            run_json(&mut shell, &["list", "--all"], Duration::from_secs(30))? == json!([]),
            "list --all after purge is not empty",
        )?;
        let generated = run_json(
            &mut shell,
            &[
                "generate", "--length", "24", "--lower", "--upper", "--digits",
            ],
            Duration::from_secs(30),
        )?;
        let password = generated["password"].as_str().unwrap_or_default();
        let password_pattern = Regex::new(r"^[A-Za-z0-9]+$").map_err(|error| error.to_string())?;
        fixture::ensure(
            password.len() == 24,
            format!("generated password length is {}, not 24", password.len()),
        )?;
        fixture::ensure(
            password_pattern.is_match(password),
            "generated password contains characters outside A-Za-z0-9",
        )?;
        shell.close()
    })();
    fixture::clean(&temp_dir);
    result
}
