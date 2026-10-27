use crate::manifest::*;

mod matrix;

/// A mapping key as text. A key that is not text names nothing a refusal or
/// a lookup could use, so it is refused at `at` rather than read as "".
fn key_text<'a>(key: &'a Value, file: &Path, at: &str) -> Result<&'a str, Failure> {
    present(key.as_str(), file, &format!("{at} keys must be text"))
}

/// The declarations around the journeys: SEO, secrets, data hooks,
/// retention, the matrix and the gate policies.
pub(crate) fn validate_policies(
    surfaces: &serde_yaml::Mapping,
    document: &Value,
    file: &Path,
    journeys: &serde_yaml::Mapping,
) -> Answer {
    let redaction = Redaction::of_document(document);
    if let Some(seo) = document.get("seo") {
        require(seo.as_mapping().is_some(), file, "seo must be an object")?;
        require(
            string_of(seo, "policy").is_some_and(|policy| !policy.is_empty()),
            file,
            "seo.policy is required",
        )?;
        require(
            string_of(seo, "brief").is_some_and(|brief| !brief.is_empty()),
            file,
            "seo.brief is required",
        )?;
        let profiles = present(map_of(seo, "profiles"), file, "seo.profiles are required")?;
        for (profile_key, profile) in profiles {
            let profile_name = key_text(profile_key, file, "seo.profiles")?;
            // The profiles are the evaluator's modes; SeoMode's refusal lists them.
            if let Err(refusal) = SeoMode::parse(profile_key) {
                require(
                    false,
                    file,
                    &format!("seo profile {profile_name} is unsupported: {refusal}"),
                )?;
            }
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
            let name = key_text(key, file, "secretRefs")?;
            require(
                reference
                    .as_str()
                    .is_some_and(|value| value.starts_with("vault://")),
                file,
                &format!("secretRefs.{name} must be a vault:// reference"),
            )?;
        }
    }

    // Every entry under `data` is a lifecycle hook the run may call.
    for (hook_key, hook) in document
        .get("data")
        .and_then(Value::as_mapping)
        .into_iter()
        .flatten()
    {
        let hook_name = key_text(hook_key, file, "data")?;
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
        // A hook that states no args passes none.
        let args_are_strings = match sequence_of(hook, "args") {
            Some(list) => list.iter().all(Value::is_string),
            None => true,
        };
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
            let name = key_text(name, file, "artifacts.retain")?;
            // A normal, positive float is a duration above zero: zero, a
            // negative, a NaN and an infinity are each refused.
            require(
                days.as_f64()
                    .is_some_and(|value| value.is_normal() && value.is_sign_positive()),
                file,
                &format!("artifacts.retain.{name} must be positive"),
            )?;
        }
    }

    matrix::validate_matrix(surfaces, document, file, &redaction)?;
    matrix::validate_gate_policies(surfaces, document, file, journeys)
}
