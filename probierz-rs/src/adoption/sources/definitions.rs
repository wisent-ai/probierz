use crate::adoption::*;

/// Validate and transactionally retain definitions from another Probierz checkout.
pub(crate) fn source_definitions(source_root: &Path) -> Result<Definitions, Failure> {
    let apps_root = source_root.join("apps");
    let apps_metadata = fs::symlink_metadata(&apps_root).map_err(|_| {
        fail(
            "adoption.source",
            format!(
                "selected repository has no supported Probierz apps directory: {}",
                apps_root.display()
            ),
        )
    })?;
    if apps_metadata.file_type().is_symlink() || !apps_metadata.is_dir() {
        return Err(fail(
            "adoption.source",
            format!(
                "selected repository has no supported Probierz apps directory: {}",
                apps_root.display()
            ),
        ));
    }

    let mut entries: Vec<_> = fs::read_dir(&apps_root)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut application_ids = Vec::new();
    let mut manifests = Vec::new();
    for entry in entries {
        let name = entry.file_name();
        if os_text(&name).starts_with('.') {
            continue;
        }
        let app_root = entry.path();
        let metadata = fs::symlink_metadata(&app_root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(fail(
                "adoption.source",
                format!(
                    "unsupported entry in Probierz apps directory: {}",
                    app_root.display()
                ),
            ));
        }
        let manifest_file = app_root.join("probierz.yaml");
        if !manifest_file.exists() {
            continue;
        }
        let manifest_metadata = fs::symlink_metadata(&manifest_file)?;
        if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
            return Err(fail(
                "adoption.source",
                format!(
                    "Probierz app directory has a non-regular probierz.yaml: {}",
                    app_root.display()
                ),
            ));
        }
        let body = fs::read_to_string(&manifest_file)?;
        let document: serde_yaml::Value = serde_yaml::from_str(&body)?;
        crate::manifest::validate(&document, &manifest_file)?;
        let declared = document
            .get("appId")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or_default();
        let expected = os_text(&name);
        if declared != expected {
            return Err(fail(
                "adoption.source",
                format!("invalid app manifest: expected appId {expected}, got {declared}"),
            ));
        }
        application_ids.push(declared.to_string());
        manifests.push((document, manifest_file));
    }
    if manifests.is_empty() {
        return Err(fail(
            "adoption.source",
            format!(
                "selected repository contains no Probierz application manifests: {}",
                apps_root.display()
            ),
        ));
    }

    let mut roots = vec!["apps".to_string()];
    let mut seen = BTreeSet::from(["apps".to_string()]);
    for package in TARGET_PACKAGES.iter().map(|(_, package)| *package) {
        for directory in SPEC_DIRECTORIES {
            let relative = format!("{package}/{directory}");
            if source_root.join(&relative).exists() && seen.insert(relative.clone()) {
                roots.push(relative);
            }
        }
    }

    let mut gathered = Vec::new();
    for root in roots {
        gathered.extend(files_below(source_root, &root)?);
    }
    let skipped_local_state: Vec<String> = gathered
        .iter()
        .filter(|file| file.relative == INDEX_RELATIVE_PATH)
        .map(|file| file.relative.clone())
        .collect();
    gathered.retain(|file| file.relative != INDEX_RELATIVE_PATH);
    let relative_files: BTreeSet<&str> =
        gathered.iter().map(|file| file.relative.as_str()).collect();

    for (document, file) in manifests {
        let Some(surfaces) = document
            .get("surfaces")
            .and_then(serde_yaml::Value::as_mapping)
        else {
            continue;
        };
        for (target, surface) in surfaces {
            let target = target.as_str().unwrap_or_default();
            let package = target_package(target).ok_or_else(|| {
                fail(
                    "adoption.source",
                    format!(
                        "invalid app manifest: {} surface {target} has no supported Probierz package",
                        file.display()
                    ),
                )
            })?;
            let declared_spec = surface
                .get("spec")
                .and_then(serde_yaml::Value::as_str)
                .unwrap_or_default();
            let package_prefix = format!("{package}/");
            let found = relative_files
                .iter()
                .filter_map(|relative| relative.strip_prefix(&package_prefix))
                .any(|relative| matches_declared_spec(declared_spec, relative));
            if !found {
                return Err(fail(
                    "adoption.source",
                    format!(
                        "invalid app manifest: {} surface {target} spec {declared_spec} was not found in {package}",
                        file.display()
                    ),
                ));
            }
        }
    }

    for file in &mut gathered {
        file.bytes = fs::read(&file.source)?;
        file.sha256 = sha256(&file.bytes);
    }
    let mut identity = Sha256::new();
    for file in &gathered {
        identity.update(file.relative.as_bytes());
        identity.update([0]);
        identity.update(file.sha256.as_bytes());
        identity.update([0]);
        identity.update(file.mode.to_string().as_bytes());
        identity.update([0]);
    }
    Ok(Definitions {
        application_ids,
        files: gathered,
        skipped_local_state,
        source_digest: hex::encode(identity.finalize()),
    })
}

