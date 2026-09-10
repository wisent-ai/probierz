use std::time::Duration;

use serde_json::json;

use crate::specs;

use crate::specs::tui::skarbiec::fixture::{self as fixture, Shell};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::binary(context);
    let temp_dir = fixture::scratch("skarbiec-manage-secrets")?;
    let vault_file = temp_dir.join("manage-secrets.vault.json");
    let audit_file = temp_dir.join("manage-secrets.audit.jsonl");
    let result = (|| {
        let env = fixture::env(&[
            ("GNUPGHOME", &temp_dir),
            ("SKARBIEC_VAULT_FILE", &vault_file),
            ("SKARBIEC_AUDIT_FILE", &audit_file),
        ]);
        let mut shell = Shell::spawn(
            "__SKARBIEC_MANAGE_SECRETS_READY__",
            "__SKARBIEC_MANAGE_SECRETS_COMMAND_",
            None,
            &env,
            120,
            36,
        )?;
        let menu =
            fixture::successful_json(&mut shell, &binary, &[], &[], Duration::from_secs(30))?;
        for command in ["init", "set", "get", "list"] {
            fixture::ensure(
                fixture::strings(&menu, "/commands").contains(&command),
                format!("command menu is missing {command}"),
            )?;
        }
        let initialized = fixture::successful_json(
            &mut shell,
            &binary,
            &["init", "manage-secrets-e2e-owner"],
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

        let secrets = [
            ("database-password", "db-secret-4f96d2"),
            ("service-api-token", "api-token-8c13ab"),
        ];
        for (id, value) in secrets {
            let field = format!("value={value}");
            let stored = fixture::successful_json(
                &mut shell,
                &binary,
                &["set", id, "--type", "note", &field],
                &[],
                Duration::from_secs(30),
            )?;
            fixture::ensure(
                stored == json!({"id": id, "kind": "note", "ok": true}),
                format!("stored answer for {id} is wrong: {stored}"),
            )?;
        }
        for (id, value) in secrets {
            let retrieved = fixture::successful_json(
                &mut shell,
                &binary,
                &["get", id],
                &[],
                Duration::from_secs(30),
            )?;
            fixture::ensure(
                retrieved
                    == json!({"schema":"skarbiec.item.v2","kind":"note","fields":{"value":value},"context":{}}),
                format!("retrieved answer for {id} is wrong: {retrieved}"),
            )?;
        }
        let listed =
            fixture::successful_json(&mut shell, &binary, &["list"], &[], Duration::from_secs(30))?;
        let items = listed
            .as_array()
            .ok_or_else(|| "list did not return an array".to_string())?;
        fixture::ensure(
            items.len() == secrets.len(),
            format!(
                "list returned {} items instead of {}",
                items.len(),
                secrets.len()
            ),
        )?;
        let mut ids = items
            .iter()
            .filter_map(|item| item["id"].as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        fixture::ensure(
            ids == vec!["database-password", "service-api-token"],
            format!("list returned the wrong ids: {ids:?}"),
        )?;
        for item in items {
            fixture::ensure(
                item["deleted"] == false,
                format!("listed item is marked deleted: {item}"),
            )?;
        }
        shell.close()
    })();
    fixture::clean(&temp_dir);
    result
}
