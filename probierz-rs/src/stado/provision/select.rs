use crate::stado::*;
pub(crate) fn select_run_provision(
    args: &RunArgs,
    app_id: &str,
    target: &str,
) -> Result<Option<Provision>, Failure> {
    if let Some(None) = args.app_binary_path {
        return Err(Failure::config(
            "stado.run",
            "--app-binary-path needs a value",
        ));
    }
    if let Some(None) = args.app_bundle_path {
        return Err(Failure::config(
            "stado.run",
            "--app-bundle-path needs a value",
        ));
    }
    let mut candidates = Vec::new();
    if let Some(Some(path)) = &args.app_binary_path {
        candidates.push((
            "--app-binary-path",
            Provision::NativeBinary {
                app_id: app_id.to_string(),
                binary_path: path.clone(),
                binary_name: None,
                binary_sha256: None,
            },
        ));
    }
    if args.cargo_release {
        candidates.push((
            "--cargo-release",
            Provision::CargoRelease {
                app_id: app_id.to_string(),
                binary: args.binary.clone().unwrap_or_else(|| app_id.to_string()),
                manifest_path: args
                    .cargo_manifest
                    .clone()
                    .unwrap_or_else(|| "Cargo.toml".to_string()),
            },
        ));
    }
    if let Some(Some(path)) = &args.app_bundle_path {
        candidates.push((
            "--app-bundle-path",
            Provision::AppBundle {
                app_id: app_id.to_string(),
                bundle_path: path.clone(),
                bundle_name: None,
            },
        ));
    }
    if args.node_source {
        candidates.push((
            "--node-source",
            Provision::NodeSource {
                app_id: app_id.to_string(),
                script: args.script.clone(),
            },
        ));
    }
    validate_provision_candidates(
        &candidates,
        args.binary.is_some(),
        args.cargo_manifest.is_some(),
        args.cargo_release,
    )?;
    if args.app_binary_path.is_some() && args.app_repo.is_none() {
        return Err(Failure::config(
            "stado.run",
            "--app-binary-path requires --app-repo <path>",
        ));
    }
    if args.app_binary_path.is_some() && target != "tui" {
        return Err(Failure::config(
            "stado.run",
            "--app-binary-path is supported only for remote TUI runs",
        ));
    }
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, provision)| provision))
}

pub(crate) fn select_author_provision(
    args: &AuthorArgs,
    app_id: &str,
    target: &str,
) -> Result<Option<Provision>, Failure> {
    for (flag, value) in [
        ("--app-path", args.app_path.as_ref()),
        ("--app-binary-path", args.app_binary_path.as_ref()),
        ("--app-bundle-path", args.app_bundle_path.as_ref()),
    ] {
        if matches!(value, Some(None)) {
            return Err(Failure::config(
                "stado.author",
                format!("{flag} needs a value"),
            ));
        }
    }
    let mut candidates = Vec::new();
    if let Some(Some(path)) = &args.app_path {
        candidates.push((
            "--app-path",
            Provision::InstalledTui {
                app_id: app_id.to_string(),
                path: path.clone(),
            },
        ));
    }
    if let Some(Some(path)) = &args.app_binary_path {
        candidates.push((
            "--app-binary-path",
            Provision::NativeBinary {
                app_id: app_id.to_string(),
                binary_path: path.clone(),
                binary_name: None,
                binary_sha256: None,
            },
        ));
    }
    if args.cargo_release {
        candidates.push((
            "--cargo-release",
            Provision::CargoRelease {
                app_id: app_id.to_string(),
                binary: args.binary.clone().unwrap_or_else(|| app_id.to_string()),
                manifest_path: args
                    .cargo_manifest
                    .clone()
                    .unwrap_or_else(|| "Cargo.toml".to_string()),
            },
        ));
    }
    if let Some(Some(path)) = &args.app_bundle_path {
        candidates.push((
            "--app-bundle-path",
            Provision::AppBundle {
                app_id: app_id.to_string(),
                bundle_path: path.clone(),
                bundle_name: None,
            },
        ));
    }
    validate_provision_candidates(
        &candidates,
        args.binary.is_some(),
        args.cargo_manifest.is_some(),
        args.cargo_release,
    )?;
    if args.app_binary_path.is_some() && args.app_repo.is_none() {
        return Err(Failure::config(
            "stado.author",
            "--app-binary-path requires --app-repo <path>",
        ));
    }
    if args.app_binary_path.is_some() && target != "tui" {
        return Err(Failure::config(
            "stado.author",
            "--app-binary-path is supported only for remote TUI authoring",
        ));
    }
    if target == "tui"
        && !candidates.iter().any(|(_, provision)| {
            matches!(
                provision,
                Provision::InstalledTui { .. }
                    | Provision::NativeBinary { .. }
                    | Provision::CargoRelease { .. }
            )
        })
    {
        return Err(Failure::config(
            "stado.author",
            "stado author --target tui needs --app-path <installed-command>, --app-binary-path <file> --app-repo <path>, or --cargo-release --app-repo <path> [--binary <name>]",
        ));
    }
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, provision)| provision))
}

pub(crate) fn validate_provision_candidates(
    candidates: &[(&str, Provision)],
    has_binary: bool,
    has_manifest: bool,
    cargo_release: bool,
) -> Result<(), Failure> {
    if candidates.len() > 1 {
        let flags = candidates
            .iter()
            .map(|(flag, _)| *flag)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Failure::config(
            "stado.run",
            format!("remote application provisioning options are mutually exclusive: {flags}"),
        ));
    }
    if (has_binary || has_manifest) && !cargo_release {
        return Err(Failure::config(
            "stado.run",
            "--binary and --cargo-manifest require --cargo-release",
        ));
    }
    Ok(())
}

