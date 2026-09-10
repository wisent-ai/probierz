use serde_json::json;
use crate::stado::*;
pub(crate) fn seo_script(
    app_id: &str,
    base_url: &str,
    mode: &str,
    policy: &str,
    brief: &str,
    primary: &str,
    secondary: &str,
    adjudicator: &str,
    agent_id: &str,
    router_url: &str,
    production_evidence: bool,
    signature_required: bool,
    hash: &str,
) -> String {
    let mut lines = vec![
        "set -euo pipefail".to_string(),
        "JOB_ROOT=\"$PWD\"".into(),
        "mkdir -p output work".into(),
        "export CARGO_HOME=\"${CARGO_HOME:-$HOME/.cargo}\"".into(),
        "export RUSTUP_HOME=\"${RUSTUP_HOME:-$HOME/.rustup}\"".into(),
        "export PATH=\"$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$CARGO_HOME/bin:$PATH\"".into(),
        "mkdir -p \"$JOB_ROOT/work/probierz\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/probierz.tar.gz\" -C \"$JOB_ROOT/work/probierz\"".into(),
        "command -v cargo >/dev/null 2>&1 || { curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal; }".into(),
        "cargo build --locked --release --manifest-path \"$JOB_ROOT/work/probierz/probierz-rs/Cargo.toml\" --bin probierz".into(),
        "PROBIERZ=\"$JOB_ROOT/work/probierz/probierz-rs/target/release/probierz\"".into(),
        "HARNESS=\"$JOB_ROOT/work/probierz\"".into(),
        format!("export STADO_MODEL_ROUTER_URL={}", shell_quote(router_url)),
        format!("export PROBIERZ_MODEL_AGENT_ID={}", shell_quote(agent_id)),
        ": \"${STADO_MODEL_ROUTER_TOKEN:?STADO_MODEL_ROUTER_TOKEN was not materialized by Stado}\"".into(),
        ": \"${PROBIERZ_MODEL_AGENT_SECRET:?PROBIERZ_MODEL_AGENT_SECRET was not materialized by Stado}\"".into(),
    ];
    if signature_required {
        lines.push(": \"${PROBIERZ_SEO_RECEIPT_PRIVATE_KEY:?PROBIERZ_SEO_RECEIPT_PRIVATE_KEY was not materialized by Stado}\"".into());
    }
    let mut command = format!(
        "\"$PROBIERZ\" --harness \"$HARNESS\" seo-evaluate --app {} --base-url {} --mode {} --policy {} --brief {} --primary-model {} --secondary-model {} --adjudicator-model {} --agent-id {}",
        shell_quote(app_id), shell_quote(base_url), shell_quote(mode), shell_quote(policy), shell_quote(brief),
        shell_quote(primary), shell_quote(secondary), shell_quote(adjudicator), shell_quote(agent_id),
    );
    if production_evidence {
        command.push_str(" --production-evidence \"$JOB_ROOT/inputs/production-evidence.json\"");
    }
    lines.extend([
        "set +e".into(),
        command,
        "PROBIERZ_SEO_RC=$?".into(),
        "set -e".into(),
        format!("tar -czf \"$JOB_ROOT/output/probierz-seo-{hash}.tar.gz\" test-results"),
        "exit $PROBIERZ_SEO_RC".into(),
    ]);
    lines.join("\n")
}

pub(crate) fn provision_inputs(
    app_id: &str,
    provision: &mut Option<Provision>,
    app_repo: Option<&Path>,
    source_required: bool,
) -> Result<Map<String, Value>, Failure> {
    let mut inputs = Map::new();
    match provision {
        Some(Provision::InstalledTui { path, .. }) => {
            if !path.is_absolute() {
                return Err(Failure::config(
                    "stado.pack",
                    "Remote installed-TUI authoring needs --app-path <absolute-path>.",
                ));
            }
            if source_required {
                let repository = app_repo.ok_or_else(|| Failure::config("stado.pack", "Remote installed-TUI authoring needs --app-repo <path> or a manifest repository root."))?;
                insert_source_input(&mut inputs, app_id, repository)?;
            }
        }
        Some(Provision::NativeBinary {
            binary_path,
            binary_name,
            binary_sha256,
            ..
        }) => {
            let repository = app_repo.ok_or_else(|| {
                Failure::config(
                    "stado.pack",
                    "Remote native-binary provisioning needs --app-repo <path>.",
                )
            })?;
            if !binary_path.is_file() {
                return Err(Failure::config("stado.pack", "The --app-binary-path you gave is not a file. Supply the signed native executable."));
            }
            let staged = work_path(&format!(
                "{app_id}-binary-{}-{}",
                now_millis(),
                std::process::id()
            ))?;
            fs::copy(&binary_path, &staged)?;
            let digest = hash_file(&staged)?;
            *binary_name = binary_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string);
            *binary_sha256 = Some(digest.clone());
            inputs.insert(
                "binary".into(),
                json!({
                    "stado_uri": upload(&staged, &format!("{app_id}-binary-{digest}"))?,
                    "relative_path": format!("inputs/{app_id}.binary"),
                }),
            );
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        Some(Provision::CargoRelease { manifest_path, .. }) => {
            let repository = app_repo.ok_or_else(|| {
                Failure::config(
                    "stado.pack",
                    "Remote cargo-release provisioning needs --app-repo <path>.",
                )
            })?;
            if !safe_relative_path(manifest_path) {
                return Err(Failure::config(
                    "stado.pack",
                    "--cargo-manifest must be a safe path relative to --app-repo.",
                ));
            }
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        Some(Provision::NodeSource { .. }) => {
            let repository = app_repo.ok_or_else(|| {
                Failure::config(
                    "stado.pack",
                    "Remote node-source provisioning needs --app-repo <path>.",
                )
            })?;
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        Some(Provision::AppBundle {
            bundle_path,
            bundle_name,
            ..
        }) => {
            if !bundle_path.exists() {
                return Err(Failure::config(
                    "stado.pack",
                    "The --app-bundle-path you gave does not exist. Build the bundle first.",
                ));
            }
            let (bundle, name) = pack_app_bundle(app_id, bundle_path)?;
            *bundle_name = Some(name);
            inputs.insert("bundle".into(), json!({
                "stado_uri": upload(&bundle.file, &format!("{app_id}-app-{}.tar.gz", bundle.hash))?,
                "relative_path": format!("inputs/{app_id}-app.tar.gz"),
            }));
            let repository = app_repo.ok_or_else(|| Failure::config(
                "stado.pack",
                format!("Remote app-bundle runs need the app source repo: pass --app-repo, or set repositories[0].root in apps/{app_id}/probierz.yaml."),
            ))?;
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        None if source_required => {
            let repository = app_repo.ok_or_else(|| {
                Failure::config(
                    "stado.pack",
                    "Remote authoring needs the product source repository.",
                )
            })?;
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        None => {}
    }
    Ok(inputs)
}

pub(crate) fn insert_source_input(inputs: &mut Map<String, Value>, app_id: &str, repository: &Path) -> Answer {
    let source = pack_app_source(app_id, repository)?;
    inputs.insert(
        "app".into(),
        json!({
            "stado_uri": upload(&source.file, &format!("{app_id}-{}.tar.gz", source.hash))?,
            "relative_path": format!("inputs/{app_id}.tar.gz"),
        }),
    );
    Ok(())
}

pub(crate) fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
        && !value.split('/').any(|part| part == "..")
}

