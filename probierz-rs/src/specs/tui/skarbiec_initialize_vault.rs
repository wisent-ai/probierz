use std::time::Duration;

use regex::Regex;

use crate::specs;

use super::skarbiec_fixture::{self as fixture, Shell};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::binary(context);
    let temp_dir = fixture::scratch("skarbiec-initialize-vault")?;
    let vault_file = temp_dir.join("fresh.vault.json");
    let result = (|| {
        let env = fixture::env(&[
            ("GNUPGHOME", &temp_dir),
            ("SKARBIEC_VAULT_FILE", &vault_file),
        ]);
        let mut shell = Shell::spawn(
            "__SKARBIEC_INITIALIZE_VAULT_READY__",
            "__SKARBIEC_INITIALIZE_COMMAND_",
            None,
            &env,
            120,
            36,
        )?;
        let menu =
            fixture::successful_json(&mut shell, &binary, &[], &[], Duration::from_secs(30))?;
        fixture::ensure(
            fixture::strings(&menu, "/commands").contains(&"init"),
            "command menu is missing init",
        )?;

        let initialized = fixture::successful_json(
            &mut shell,
            &binary,
            &["init", "initialize-vault-e2e-owner"],
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
        let fingerprint = Regex::new(r"^[0-9A-F]{40}$").map_err(|error| error.to_string())?;
        let owner = initialized["owner_fpr"].as_str().unwrap_or_default();
        let recovery = initialized["recovery_fpr"].as_str().unwrap_or_default();
        fixture::ensure(
            fingerprint.is_match(owner),
            "owner fingerprint is not 40 uppercase hexadecimal characters",
        )?;
        fixture::ensure(
            fingerprint.is_match(recovery),
            "recovery fingerprint is not 40 uppercase hexadecimal characters",
        )?;
        fixture::ensure(
            owner != recovery,
            "owner and recovery fingerprints must differ",
        )?;
        shell.close()
    })();
    fixture::clean(&temp_dir);
    result
}
