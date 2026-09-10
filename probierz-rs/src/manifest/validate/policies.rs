use crate::manifest::*;

/// The declarations around the journeys: SEO, secrets, data hooks,
/// retention, the matrix and the gate policies.
pub(crate) fn validate_policies(
    surfaces: &serde_yaml::Mapping,
    document: &Value,
    file: &Path,
    journeys: &serde_yaml::Mapping,
) -> Answer {
    if let Some(seo) = document.get("seo") {
        require(seo.as_mapping().is_some(), file, "seo must be an object")?;
        require(
            !string_of(seo, "policy").unwrap_or_default().is_empty(),
            file,
            "seo.policy is required",
        )?;
        require(
            !string_of(seo, "brief").unwrap_or_default().is_empty(),
            file,
            "seo.brief is required",
        )?;
        let profiles = map_of(seo, "profiles");
        require(profiles.is_some(), file, "seo.profiles are required")?;
        for (profile_key, profile) in profiles.expect("checked") {
            let profile_name = profile_key.as_str().unwrap_or_default();
            require(
                matches!(
                    profile_name,
                    "pull-request" | "release" | "nightly" | "production"
                ),
                file,
                &format!("seo profile {profile_name} is unsupported"),
            )?;
            require(
                profile.as_mapping().is_some(),
                file,
                &format!("seo.profiles.{profile_name} must be an object"),
            )?;
            require(
                profile
                    .get("requireSignature")
                    .and_then(Value::as_bool)
                    .is_some(),
                file,
                &format!("seo.profiles.{profile_name}.requireSignature must be boolean"),
            )?;
            require(
                profile
                    .get("requireProductionEvidence")
                    .and_then(Value::as_bool)
                    .is_some(),
                file,
                &format!("seo.profiles.{profile_name}.requireProductionEvidence must be boolean"),
            )?;
        }
    }

    if let Some(refs) = map_of(document, "secretRefs") {
        for (key, reference) in refs {
            let name = key.as_str().unwrap_or_default();
            require(
                reference
                    .as_str()
                    .map(|value| value.starts_with("vault://"))
                    .unwrap_or(false),
                file,
                &format!("secretRefs.{name} must be a vault:// reference"),
            )?;
        }
    }

    for hook_name in ["seed", "cleanup"] {
        let hook = document.get("data").and_then(|data| data.get(hook_name));
        let Some(hook) = hook else { continue };
        let command = string_of(hook, "command").filter(|value| !value.is_empty());
        let capability = string_of(hook, "capability").filter(|value| !value.is_empty());
        require(
            command.is_some() ^ capability.is_some(),
            file,
            &format!("data.{hook_name} must name exactly one command or capability"),
        )?;
        if let Some(capability) = capability {
            require(
                crate::apphooks::supports(capability),
                file,
                &format!("data.{hook_name} capability {capability} is unknown"),
            )?;
        }
        let args_are_strings = sequence_of(hook, "args")
            .map(|list| list.iter().all(Value::is_string))
            .unwrap_or(true);
        require(
            args_are_strings,
            file,
            &format!("data.{hook_name}.args must be strings"),
        )?;
    }

    if let Some(retain) = document
        .get("artifacts")
        .and_then(|artifacts| map_of(artifacts, "retain"))
    {
        for (name, days) in retain {
            let name = name.as_str().unwrap_or_default();
            require(
                days.as_f64().map(|value| value > 0.0).unwrap_or(false),
                file,
                &format!("artifacts.retain.{name} must be positive"),
            )?;
        }
    }

    if let Some(matrix) = map_of(document, "matrix") {
        for (profile_key, profile) in matrix {
            let profile_name = profile_key.as_str().unwrap_or_default();
            require(
                profile.as_mapping().is_some(),
                file,
                &format!("matrix.{profile_name} must be an object"),
            )?;
            let targets = sequence_of(profile, "targets");
            require(
                targets.map(|list| !list.is_empty()).unwrap_or(false),
                file,
                &format!("matrix.{profile_name}.targets are required"),
            )?;
            for target in targets.expect("checked") {
                let target = target.as_str().unwrap_or_default();
                require(
                    surfaces.get(Value::from(target)).is_some(),
                    file,
                    &format!("matrix.{profile_name} target {target} has no surface"),
                )?;
            }
            if let Some(dimensions) = map_of(profile, "dimensions") {
                for (name, values) in dimensions {
                    let name = name.as_str().unwrap_or_default();
                    require(
                        !sensitive(name),
                        file,
                        &format!(
                            "matrix.{profile_name} secret dimension {name} must use secretRefs"
                        ),
                    )?;
                    let list = values.as_sequence();
                    require(
                        list.map(|items| !items.is_empty()).unwrap_or(false),
                        file,
                        &format!("matrix.{profile_name}.{name} needs values"),
                    )?;
                    let scalar = list
                        .expect("checked")
                        .iter()
                        .all(|value| value.is_string() || value.is_number() || value.is_bool());
                    require(
                        scalar,
                        file,
                        &format!("matrix.{profile_name}.{name} values must be scalar"),
                    )?;
                }
            }
            let evidence = string_of(profile, "minimumCellEvidence").unwrap_or("E3");
            require(
                matches!(evidence, "E2" | "E3"),
                file,
                &format!("matrix.{profile_name}.minimumCellEvidence must be E2 or E3"),
            )?;
            let encryption = string_of(profile, "artifactEncryption").unwrap_or("optional");
            require(
                matches!(encryption, "optional" | "required"),
                file,
                &format!("matrix.{profile_name}.artifactEncryption must be optional or required"),
            )?;
            let remove_plaintext = profile.get("removePlaintextAfterProtection");
            require(
                remove_plaintext.is_none() || remove_plaintext.and_then(Value::as_bool).is_some(),
                file,
                &format!("matrix.{profile_name}.removePlaintextAfterProtection must be boolean"),
            )?;
            let max_cells = profile
                .get("maxCells")
                .and_then(Value::as_f64)
                .unwrap_or(128.0);
            require(
                max_cells > 0.0,
                file,
                &format!("matrix.{profile_name}.maxCells must be positive"),
            )?;
            let parallel = profile
                .get("maximumParallel")
                .and_then(Value::as_f64)
                .unwrap_or(4.0);
            require(
                parallel > 0.0,
                file,
                &format!("matrix.{profile_name}.maximumParallel must be positive"),
            )?;
        }
    }

    for policy_name in ["pullRequestPolicy", "releasePolicy"] {
        let Some(policy) = document.get(policy_name) else {
            continue;
        };
        let evidence = string_of(policy, "minimumEvidence").unwrap_or("E3");
        require(
            matches!(evidence, "E2" | "E3"),
            file,
            &format!("{policy_name}.minimumEvidence must be E2 or E3"),
        )?;
        for flag in ["requireProtectedArtifacts", "requireSecretScan"] {
            let value = policy.get(flag);
            require(
                value.is_none() || value.and_then(Value::as_bool).is_some(),
                file,
                &format!("{policy_name}.{flag} must be boolean"),
            )?;
        }
        for target in sequence_of(policy, "requiredTargets").unwrap_or(&Vec::new()) {
            let target = target.as_str().unwrap_or_default();
            require(
                surfaces.get(Value::from(target)).is_some(),
                file,
                &format!("{policy_name} target {target} has no surface"),
            )?;
        }
        for journey in sequence_of(policy, "requiredJourneys").unwrap_or(&Vec::new()) {
            let name = journey.as_str().unwrap_or_default();
            require(
                journeys.get(Value::from(name)).is_some(),
                file,
                &format!("{policy_name} journey {name} is unknown"),
            )?;
        }
        if let Some(profile) = string_of(policy, "requiredMatrixProfile") {
            let known = map_of(document, "matrix")
                .and_then(|matrix| matrix.get(Value::from(profile)))
                .is_some();
            require(
                known,
                file,
                &format!("{policy_name} matrix {profile} is unknown"),
            )?;
        }
    }

    Ok(())
}
