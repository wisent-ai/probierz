use crate::manifest::*;

use super::key_text;

/// The test matrix: each profile's targets, dimensions, per-surface
/// overrides, evidence floor and the bounds and protections it states.
pub(super) fn validate_matrix(
    surfaces: &serde_yaml::Mapping,
    document: &Value,
    file: &Path,
    redaction: &Redaction,
) -> Answer {
    let Some(matrix) = map_of(document, "matrix") else {
        return Ok(());
    };
    for (profile_key, profile) in matrix {
        let profile_name = key_text(profile_key, file, "matrix")?;
        require(
            profile.as_mapping().is_some(),
            file,
            &format!("matrix.{profile_name} must be an object"),
        )?;
        let targets = present(
            sequence_of(profile, "targets").filter(|list| !list.is_empty()),
            file,
            &format!("matrix.{profile_name}.targets are required"),
        )?;
        for target in targets {
            let target = present(
                target.as_str(),
                file,
                &format!("matrix.{profile_name}.targets entries must be text"),
            )?;
            require(
                surfaces.get(Value::from(target)).is_some(),
                file,
                &format!("matrix.{profile_name} target {target} has no surface"),
            )?;
        }
        validate_dimensions(profile, file, &format!("matrix.{profile_name}"), redaction)?;
        if let Some(overrides) = profile.get("surfaces") {
            let overrides = present(
                overrides.as_mapping(),
                file,
                &format!("matrix.{profile_name}.surfaces must be an object"),
            )?;
            for (target, declaration) in overrides {
                require(
                    surfaces.contains_key(target),
                    file,
                    &format!("matrix.{profile_name}.surfaces names an unknown surface"),
                )?;
                let target = key_text(target, file, &format!("matrix.{profile_name}.surfaces"))?;
                let path = format!("matrix.{profile_name}.surfaces.{target}");
                require(
                    declaration.is_mapping(),
                    file,
                    &format!("{path} must be an object"),
                )?;
                validate_dimensions(declaration, file, &path, redaction)?;
            }
        }
        // The evidence floor is the profile's own statement; none is assumed.
        let evidence = string_of(profile, "minimumCellEvidence");
        require(
            matches!(evidence, Some("E2" | "E3")),
            file,
            &format!("matrix.{profile_name}.minimumCellEvidence must be stated as E2 or E3"),
        )?;
        if let Some(encryption) = profile.get("artifactEncryption") {
            if let Err(error) = serde_yaml::from_value::<Encryption>(encryption.clone()) {
                require(
                    false,
                    file,
                    &format!(
                        "matrix.{profile_name}.artifactEncryption must be optional or required: {error}"
                    ),
                )?;
            }
        }
        let remove_plaintext = profile.get("removePlaintextAfterProtection");
        require(
            remove_plaintext.is_none() || remove_plaintext.and_then(Value::as_bool).is_some(),
            file,
            &format!("matrix.{profile_name}.removePlaintextAfterProtection must be boolean"),
        )?;
        // A bound the profile states must be a positive whole number; an
        // unstated one bounds nothing.
        if let Some(value) = profile.get("maxCells") {
            require(
                value.as_u64().and_then(std::num::NonZeroU64::new).is_some(),
                file,
                &format!("matrix.{profile_name}.maxCells must be a positive whole number"),
            )?;
        }
        if let Some(value) = profile.get("maximumParallel") {
            require(
                value.as_u64().and_then(std::num::NonZeroU64::new).is_some(),
                file,
                &format!("matrix.{profile_name}.maximumParallel must be a positive whole number"),
            )?;
        }
    }
    Ok(())
}

/// A gate policy is every top-level declaration named `<gate>Policy`; its
/// flags are the booleans `GateFlags` names.
pub(super) fn validate_gate_policies(
    surfaces: &serde_yaml::Mapping,
    document: &Value,
    file: &Path,
    journeys: &serde_yaml::Mapping,
) -> Answer {
    for (policy_key, policy) in document.as_mapping().into_iter().flatten() {
        let policy_name = key_text(policy_key, file, "the manifest")?;
        if !policy_name.ends_with("Policy") {
            continue;
        }
        require(
            matches!(string_of(policy, "minimumEvidence"), Some("E2" | "E3")),
            file,
            &format!("{policy_name}.minimumEvidence must be E2 or E3"),
        )?;
        if let Err(error) = serde_yaml::from_value::<GateFlags>(policy.clone()) {
            require(
                false,
                file,
                &format!("{policy_name} flags must be boolean: {error}"),
            )?;
        }
        for target in sequence_of(policy, "requiredTargets").into_iter().flatten() {
            let target = present(
                target.as_str(),
                file,
                &format!("{policy_name}.requiredTargets entries must be text"),
            )?;
            require(
                surfaces.get(Value::from(target)).is_some(),
                file,
                &format!("{policy_name} target {target} has no surface"),
            )?;
        }
        for journey in sequence_of(policy, "requiredJourneys")
            .into_iter()
            .flatten()
        {
            let name = present(
                journey.as_str(),
                file,
                &format!("{policy_name}.requiredJourneys entries must be text"),
            )?;
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

fn validate_dimensions(
    declaration: &Value,
    file: &Path,
    path: &str,
    redaction: &Redaction,
) -> Answer {
    let Some(dimensions) = declaration.get("dimensions") else {
        return Ok(());
    };
    let dimensions = present(
        dimensions.as_mapping(),
        file,
        &format!("{path}.dimensions must be an object"),
    )?;
    for (name, values) in dimensions {
        let name = key_text(name, file, &format!("{path}.dimensions"))?;
        let field = format!("{path}.dimensions.{name}");
        require(
            !redaction.covers(name),
            file,
            &format!("{field} must use secretRefs"),
        )?;
        let list = present(
            values.as_sequence().filter(|items| !items.is_empty()),
            file,
            &format!("{field} needs values"),
        )?;
        require(
            list.iter()
                .all(|value| value.is_string() || value.is_number() || value.is_bool()),
            file,
            &format!("{field} values must be scalar"),
        )?;
    }
    Ok(())
}

/// What a matrix profile states about protecting its artifacts.
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
enum Encryption {
    Optional,
    Required,
}

/// The boolean flags a gate policy may state; read only to require that
/// each one stated is a boolean.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GateFlags {
    require_protected_artifacts: Option<bool>,
    require_secret_scan: Option<bool>,
}
