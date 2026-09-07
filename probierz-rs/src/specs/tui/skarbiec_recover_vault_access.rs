use std::fs;
use std::time::Duration;

use regex::Regex;
use serde_json::json;

use crate::specs;

use super::skarbiec_fixture::{self as fixture, Shell};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::binary(context);
    let temp_dir = fixture::scratch("skarbiec-recover-vault-access")?;
    let owner_keyring = temp_dir.join("owner-keyring");
    let recovered_keyring = temp_dir.join("recovered-keyring");
    let recovery_backup = temp_dir.join("recovery-private-key.asc");
    let vault_file = temp_dir.join("recovery-journey.vault.json");
    let audit_file = temp_dir.join("recovery-journey.audit.jsonl");
    fs::create_dir_all(&owner_keyring)
        .map_err(|error| format!("{}: {error}", owner_keyring.display()))?;
    fs::create_dir_all(&recovered_keyring)
        .map_err(|error| format!("{}: {error}", recovered_keyring.display()))?;
    let result = (|| {
        let env = fixture::env(&[
            ("GNUPGHOME", &owner_keyring),
            ("SKARBIEC_VAULT_FILE", &vault_file),
            ("SKARBIEC_AUDIT_FILE", &audit_file),
        ]);
        let mut shell = Shell::spawn(
            "__SKARBIEC_RECOVER_VAULT_ACCESS_READY__",
            "__SKARBIEC_RECOVERY_COMMAND_",
            None,
            &env,
            120,
            36,
        )?;
        let run_json = |shell: &mut Shell,
                        args: &[&str],
                        keyring: &std::path::Path,
                        timeout: Duration|
         -> Result<serde_json::Value, String> {
            let description = if args.is_empty() {
                "command menu".to_string()
            } else {
                args.join(" ")
            };
            let keyring_text = keyring.to_string_lossy();
            let result = shell.run_program(
                &binary,
                args,
                &[("GNUPGHOME", keyring_text.as_ref())],
                timeout,
            )?;
            fixture::ensure(result.status == 0, format!("{description} failed"))?;
            fixture::parse_json(&result.output, || {
                format!("expected JSON output from {description}")
            })
        };
        let menu = run_json(&mut shell, &[], &owner_keyring, Duration::from_secs(30))?;
        for command in ["init", "set", "get", "recovery-status"] {
            fixture::ensure(
                fixture::strings(&menu, "/commands").contains(&command),
                format!("expected command menu to include {command}"),
            )?;
        }
        let owner_uid = "recover-vault-access-e2e-owner";
        let secret_id = "recovery-proof-note";
        let secret_value = "vault-access-restored-73d9c1";
        let initialized = run_json(
            &mut shell,
            &["init", owner_uid],
            &owner_keyring,
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
        let fingerprint = Regex::new(r"^[0-9A-F]{40}$").map_err(|error| error.to_string())?;
        let owner_fpr = initialized["owner_fpr"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let recovery_fpr = initialized["recovery_fpr"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        fixture::ensure(
            fingerprint.is_match(&owner_fpr),
            "owner fingerprint is not 40 uppercase hexadecimal characters",
        )?;
        fixture::ensure(
            fingerprint.is_match(&recovery_fpr),
            "recovery fingerprint is not 40 uppercase hexadecimal characters",
        )?;
        fixture::ensure(
            owner_fpr != recovery_fpr,
            "owner and recovery fingerprints must differ",
        )?;
        let field = format!("value={secret_value}");
        let stored = run_json(
            &mut shell,
            &["set", secret_id, "--type", "note", &field],
            &owner_keyring,
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            stored == json!({"id":secret_id,"kind":"note","ok":true}),
            format!("stored answer is wrong: {stored}"),
        )?;
        let status = run_json(
            &mut shell,
            &["recovery-status"],
            &owner_keyring,
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            status["recovery_fpr"] == recovery_fpr && status["item_count"] == 1,
            format!("recovery status is wrong: {status}"),
        )?;
        let note = status["note"].as_str().unwrap_or_default().to_lowercase();
        fixture::ensure(
            note.contains("shares one failure domain with the owner key"),
            format!("recovery status note is wrong: {}", status["note"]),
        )?;
        let expected_secret = json!({"schema":"skarbiec.item.v2","kind":"note","fields":{"value":secret_value},"context":{}});
        fixture::ensure(
            run_json(
                &mut shell,
                &["get", secret_id],
                &owner_keyring,
                Duration::from_secs(30),
            )? == expected_secret,
            "owner keyring cannot decrypt the item",
        )?;

        let export = [
            "env".to_string(),
            format!("GNUPGHOME={}", owner_keyring.display()),
            "gpg".to_string(),
            "--batch".to_string(),
            "--yes".to_string(),
            "--armor".to_string(),
            "--output".to_string(),
            recovery_backup.to_string_lossy().into_owned(),
            "--export-secret-keys".to_string(),
            recovery_fpr.clone(),
        ]
        .iter()
        .map(|part| fixture::shell_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
        let exported = shell.run_command(&export, Duration::from_secs(30))?;
        fixture::ensure(exported.status == 0, "recovery-key backup failed")?;
        let import = [
            "env".to_string(),
            format!("GNUPGHOME={}", recovered_keyring.display()),
            "gpg".to_string(),
            "--batch".to_string(),
            "--yes".to_string(),
            "--import".to_string(),
            recovery_backup.to_string_lossy().into_owned(),
        ]
        .iter()
        .map(|part| fixture::shell_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
        let imported = shell.run_command(&import, Duration::from_secs(30))?;
        fixture::ensure(imported.status == 0, "recovery-key import failed")?;
        let inspect = [
            "env".to_string(),
            format!("GNUPGHOME={}", recovered_keyring.display()),
            "gpg".to_string(),
            "--batch".to_string(),
            "--with-colons".to_string(),
            "--list-secret-keys".to_string(),
        ]
        .iter()
        .map(|part| fixture::shell_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
        let recovered_keys = shell.run_command(&inspect, Duration::from_secs(30))?;
        fixture::ensure(
            recovered_keys.status == 0,
            "recovered keyring inspection failed",
        )?;
        fixture::ensure(
            recovered_keys.output.contains(&recovery_fpr),
            "recovered keyring does not contain the recovery fingerprint",
        )?;
        fixture::ensure(
            !recovered_keys.output.contains(&owner_fpr),
            "recovered keyring unexpectedly contains the owner fingerprint",
        )?;
        let recovered_status = run_json(
            &mut shell,
            &["recovery-status"],
            &recovered_keyring,
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            recovered_status["recovery_fpr"] == recovery_fpr && recovered_status["item_count"] == 1,
            format!("recovered-keyring status is wrong: {recovered_status}"),
        )?;
        fixture::ensure(
            run_json(
                &mut shell,
                &["get", secret_id],
                &recovered_keyring,
                Duration::from_secs(30),
            )? == expected_secret,
            "recovery identity did not restore access to the encrypted item",
        )?;
        shell.close()
    })();
    fixture::clean(&temp_dir);
    result
}
