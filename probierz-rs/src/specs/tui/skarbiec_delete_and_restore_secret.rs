use std::time::Duration;

use serde_json::json;

use crate::specs;

use super::skarbiec_fixture::{self as fixture, Shell};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::binary(context);
    let temp_dir = fixture::scratch("skarbiec-delete-restore")?;
    let vault_file = temp_dir.join("journey.vault.json");
    let audit_file = temp_dir.join("journey.audit.jsonl");
    let result = (|| {
        let env = fixture::env(&[
            ("GNUPGHOME", &temp_dir),
            ("SKARBIEC_VAULT_FILE", &vault_file),
            ("SKARBIEC_AUDIT_FILE", &audit_file),
        ]);
        let mut shell = Shell::spawn(
            "__SKARBIEC_PTY_READY__",
            "__SKARBIEC_COMMAND_",
            None,
            &env,
            120,
            36,
        )?;
        let help =
            fixture::successful_json(&mut shell, &binary, &["help"], &[], Duration::from_secs(30))?;
        fixture::ensure(
            fixture::strings(&help, "/commands").contains(&"delete"),
            "help is missing delete",
        )?;
        fixture::ensure(
            fixture::strings(&help, "/commands").contains(&"restore"),
            "help is missing restore",
        )?;
        let initialized = fixture::successful_json(
            &mut shell,
            &binary,
            &["init", "delete-restore-e2e-owner"],
            &[],
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
        let id = "recoverable-note";
        let value = "restored-secret-value-7f31";
        let field = format!("value={value}");
        let created = fixture::successful_json(
            &mut shell,
            &binary,
            &["set", id, "--type", "note", &field],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            created == json!({"id":id,"kind":"note","ok":true}),
            format!("created answer is wrong: {created}"),
        )?;
        let live =
            fixture::successful_json(&mut shell, &binary, &["list"], &[], Duration::from_secs(30))?;
        fixture::ensure(
            live.as_array().map(Vec::len) == Some(1),
            format!("live list is wrong: {live}"),
        )?;
        fixture::ensure(
            live[0]["id"] == id && live[0]["deleted"] == false,
            format!("live item is wrong: {}", live[0]),
        )?;
        let deleted = fixture::successful_json(
            &mut shell,
            &binary,
            &["delete", id],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            deleted == json!({"ok":true}),
            format!("delete answer is wrong: {deleted}"),
        )?;
        let after_delete =
            fixture::successful_json(&mut shell, &binary, &["list"], &[], Duration::from_secs(30))?;
        fixture::ensure(
            after_delete == json!([]),
            format!("live list after delete is not empty: {after_delete}"),
        )?;
        let trash = fixture::successful_json(
            &mut shell,
            &binary,
            &["list", "--all"],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            trash.as_array().map(Vec::len) == Some(1),
            format!("trash is wrong: {trash}"),
        )?;
        fixture::ensure(
            trash[0]["id"] == id && trash[0]["deleted"] == true,
            format!("trashed item is wrong: {}", trash[0]),
        )?;
        let restored = fixture::successful_json(
            &mut shell,
            &binary,
            &["restore", id],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            restored == json!({"ok":true}),
            format!("restore answer is wrong: {restored}"),
        )?;
        let restored_list =
            fixture::successful_json(&mut shell, &binary, &["list"], &[], Duration::from_secs(30))?;
        fixture::ensure(
            restored_list.as_array().map(Vec::len) == Some(1),
            format!("live list after restore is wrong: {restored_list}"),
        )?;
        fixture::ensure(
            restored_list[0]["id"] == id && restored_list[0]["deleted"] == false,
            format!("restored item is wrong: {}", restored_list[0]),
        )?;
        let recovered = fixture::successful_json(
            &mut shell,
            &binary,
            &["get", id],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            recovered
                == json!({"schema":"skarbiec.item.v2","kind":"note","fields":{"value":value},"context":{}}),
            format!("recovered secret is wrong: {recovered}"),
        )?;
        shell.close()
    })();
    fixture::clean(&temp_dir);
    result
}
