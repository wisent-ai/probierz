use std::time::Duration;

use regex::Regex;
use serde_json::json;

use crate::specs;

use super::skarbiec_fixture::{self as fixture, Shell};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::binary(context);
    let temp_dir = fixture::scratch("skarbiec-manage-users-sharing")?;
    let vault_file = temp_dir.join("manage-users-and-sharing.vault.json");
    let audit_file = temp_dir.join("manage-users-and-sharing.audit.jsonl");
    let result = (|| {
        let env = fixture::env(&[
            ("GNUPGHOME", &temp_dir),
            ("SKARBIEC_VAULT_FILE", &vault_file),
            ("SKARBIEC_AUDIT_FILE", &audit_file),
        ]);
        let mut shell = Shell::spawn(
            "__SKARBIEC_MANAGE_USERS_SHARING_READY__",
            "__SKARBIEC_MANAGE_USERS_SHARING_COMMAND_",
            None,
            &env,
            120,
            36,
        )?;
        let menu =
            fixture::successful_json(&mut shell, &binary, &[], &[], Duration::from_secs(30))?;
        for command in ["init", "set", "add-user", "share", "users", "revoke"] {
            fixture::ensure(
                fixture::strings(&menu, "/commands").contains(&command),
                format!("expected command menu to include {command}"),
            )?;
        }
        let owner = "sharing-journey-owner";
        let member = "sharing-journey-member";
        let secret_id = "shared-deployment-note";
        let secret_value = "deployment-secret-51c9e7";
        let initialized = fixture::successful_json(
            &mut shell,
            &binary,
            &["init", owner],
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
        let field = format!("value={secret_value}");
        let stored = fixture::successful_json(
            &mut shell,
            &binary,
            &["set", secret_id, "--type", "note", &field],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            stored == json!({"id":secret_id,"kind":"note","ok":true}),
            format!("stored answer is wrong: {stored}"),
        )?;
        let added = fixture::successful_json(
            &mut shell,
            &binary,
            &["add-user", member, "--role", "member"],
            &[],
            Duration::from_secs(120),
        )?;
        fixture::ensure(
            added["ok"] == true && added["uid"] == member && added["role"] == "member",
            format!("add-user answer is wrong: {added}"),
        )?;
        let fingerprint = Regex::new(r"^[0-9A-F]{40}$").map_err(|error| error.to_string())?;
        let member_fingerprint = added["fingerprint"].as_str().unwrap_or_default();
        fixture::ensure(
            fingerprint.is_match(member_fingerprint),
            "member fingerprint is not 40 uppercase hexadecimal characters",
        )?;
        let shared = fixture::successful_json(
            &mut shell,
            &binary,
            &["share", secret_id, member],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            shared["ok"] == true
                && shared["item"] == secret_id
                && shared["recipients"] == json!([member]),
            format!("share answer is wrong: {shared}"),
        )?;
        let users = fixture::successful_json(
            &mut shell,
            &binary,
            &["users"],
            &[],
            Duration::from_secs(30),
        )?;
        let mut names = users
            .as_object()
            .map(|map| map.keys().map(String::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        names.sort_unstable();
        fixture::ensure(
            names == vec![member, owner],
            format!("users returned the wrong users: {names:?}"),
        )?;
        fixture::ensure(
            users[owner]["role"] == "owner",
            format!("owner role is wrong: {}", users[owner]),
        )?;
        fixture::ensure(
            fingerprint.is_match(users[owner]["fingerprint"].as_str().unwrap_or_default()),
            "owner fingerprint is not 40 uppercase hexadecimal characters",
        )?;
        fixture::ensure(
            users[member]["role"] == "member" && users[member]["fingerprint"] == member_fingerprint,
            format!("member record is wrong: {}", users[member]),
        )?;
        let timestamp = Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
            .map_err(|error| error.to_string())?;
        fixture::ensure(
            timestamp.is_match(users[member]["added_at"].as_str().unwrap_or_default()),
            "member added_at is not a UTC timestamp",
        )?;
        let revoked = fixture::successful_json(
            &mut shell,
            &binary,
            &["revoke", secret_id, member],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            revoked["ok"] == true
                && revoked["item"] == secret_id
                && revoked["recipients"] == json!([]),
            format!("revoke answer is wrong: {revoked}"),
        )?;
        let readable = fixture::successful_json(
            &mut shell,
            &binary,
            &["get", secret_id],
            &[],
            Duration::from_secs(30),
        )?;
        fixture::ensure(
            readable
                == json!({"schema":"skarbiec.item.v2","kind":"note","fields":{"value":secret_value},"context":{}}),
            format!("owner cannot still read the item: {readable}"),
        )?;
        shell.close()
    })();
    fixture::clean(&temp_dir);
    result
}
