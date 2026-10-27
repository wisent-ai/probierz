use crate::manifest::*;

/// The text `value` holds when it is non-empty text.
fn stated(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.is_empty())
}

/// The list `value` holds when it is a non-empty sequence.
fn listed(value: Option<&Vec<Value>>) -> Option<&Vec<Value>> {
    value.filter(|list| !list.is_empty())
}

/// Refuse the manifest at `file` with `sentence`.
fn refuse(file: &Path, sentence: &str) -> Answer {
    require(false, file, sentence)
}

/// The repositories a manifest maps and the surfaces it declares. A plain
/// value a surface states may not sit under a name the manifest's own
/// redaction hides: that value belongs in `secretRefs`.
pub(crate) fn validate_repositories_and_surfaces(
    file: &Path,
    repositories: &[Value],
    surfaces: &serde_yaml::Mapping,
    journeys: &serde_yaml::Mapping,
    redaction: &Redaction,
) -> Answer {
    for repository in repositories {
        let Some(root) = stated(string_of(repository, "root")) else {
            return refuse(file, "repository root must be absolute or under ~/");
        };
        require(
            root.starts_with("~/") || Path::new(root).is_absolute(),
            file,
            "repository root must be absolute or under ~/",
        )?;
        require(
            repository
                .get("mappings")
                .and_then(Value::as_sequence)
                .is_some(),
            file,
            "repository mappings are required",
        )?;
    }

    for (target_key, surface) in surfaces {
        let Some(target) = target_key.as_str() else {
            return refuse(file, &format!("surface name {target_key:?} must be text"));
        };
        require(
            surface.as_mapping().is_some(),
            file,
            &format!("surface {target} must be an object"),
        )?;
        require(
            stated(string_of(surface, "spec")).is_some(),
            file,
            &format!("surface {target} spec is required"),
        )?;
        let Some(surface_journeys) = listed(sequence_of(surface, "journeys")) else {
            return refuse(file, &format!("surface {target} journeys are required"));
        };
        for journey in surface_journeys {
            let Some(name) = journey.as_str() else {
                return refuse(file, &format!("surface {target} journey {journey:?} must be a name"));
            };
            require(
                journeys.get(Value::from(name)).is_some(),
                file,
                &format!("surface {target} journey {name} is unknown"),
            )?;
        }
        if let Some(runner) = surface.get("runner") {
            require(
                runner.as_mapping().is_some(),
                file,
                &format!("surface {target} runner must be an object"),
            )?;
            let command = stated(string_of(runner, "command"));
            let capability = stated(string_of(runner, "capability"));
            require(
                command.is_some() ^ capability.is_some(),
                file,
                &format!("surface {target} runner must name exactly one command or capability"),
            )?;
            if let Some(capability) = capability {
                require(
                    crate::apphooks::supports(capability),
                    file,
                    &format!("surface {target} runner capability {capability} is unknown"),
                )?;
            }
            // A runner without args passes none; args it states are strings.
            if let Some(args) = sequence_of(runner, "args") {
                require(
                    args.iter().all(Value::is_string),
                    file,
                    &format!("surface {target} runner args must be strings"),
                )?;
            }
        }
        if let Some(overrides) = sequence_of(surface, "journeyOverrides") {
            for (index, override_entry) in overrides.iter().enumerate() {
                validate_override(file, target, index, override_entry, journeys, redaction)?;
            }
        }
        if let Some(conditions) = map_of(surface, "conditions") {
            for name in key_names(conditions) {
                require(
                    !redaction.covers(&name),
                    file,
                    &format!("secret condition {name} must use secretRefs"),
                )?;
            }
        }
        if let Some(env) = map_of(surface, "env") {
            for (target_name, source_name) in env {
                require(
                    stated(target_name.as_str()).is_some(),
                    file,
                    &format!("surface {target} env target is required"),
                )?;
                require(
                    stated(source_name.as_str()).is_some(),
                    file,
                    &format!("surface {target} env source is required"),
                )?;
            }
        }
    }
    Ok(())
}

/// One `journeyOverrides` entry: non-empty scalar conditions on names the
/// redaction does not hide, and a non-empty list of known journeys.
fn validate_override(
    file: &Path,
    target: &str,
    index: usize,
    override_entry: &Value,
    journeys: &serde_yaml::Mapping,
    redaction: &Redaction,
) -> Answer {
    require(
        override_entry.as_mapping().is_some(),
        file,
        &format!("surface {target} journeyOverrides.{index} must be an object"),
    )?;
    let Some(when) = map_of(override_entry, "when").filter(|map| !map.is_empty()) else {
        return refuse(file, &format!("surface {target} journeyOverrides.{index}.when is required"));
    };
    let Some(override_journeys) = listed(sequence_of(override_entry, "journeys")) else {
        return refuse(
            file,
            &format!("surface {target} journeyOverrides.{index}.journeys are required"),
        );
    };
    for (key, value) in when {
        let Some(name) = key.as_str() else {
            return refuse(file, &format!("surface {target} journey override {key:?} must be a name"));
        };
        require(
            !redaction.covers(name),
            file,
            &format!("surface {target} journey override {name} must not be sensitive"),
        )?;
        let scalar = value.is_string() || value.is_number() || value.is_bool();
        require(
            scalar,
            file,
            &format!("surface {target} journey override {name} must be scalar"),
        )?;
    }
    for journey in override_journeys {
        let Some(name) = journey.as_str() else {
            return refuse(
                file,
                &format!("surface {target} journey override {journey:?} must be a name"),
            );
        };
        require(
            journeys.get(Value::from(name)).is_some(),
            file,
            &format!("surface {target} journey override {name} is unknown"),
        )?;
    }
    Ok(())
}
