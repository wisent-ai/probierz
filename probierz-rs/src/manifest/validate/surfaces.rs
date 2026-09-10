use crate::manifest::*;

/// The repositories a manifest maps and the surfaces it declares.
pub(crate) fn validate_repositories_and_surfaces(
    file: &Path,
    repositories: &[Value],
    surfaces: &serde_yaml::Mapping,
    journeys: &serde_yaml::Mapping,
) -> Answer {
    for repository in repositories {
        let root = string_of(repository, "root").unwrap_or_default();
        require(
            !root.is_empty() && Path::new(root).is_absolute(),
            file,
            "repository root must be absolute",
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
        let target = target_key.as_str().unwrap_or_default();
        require(
            surface.as_mapping().is_some(),
            file,
            &format!("surface {target} must be an object"),
        )?;
        require(
            !string_of(surface, "spec").unwrap_or_default().is_empty(),
            file,
            &format!("surface {target} spec is required"),
        )?;
        let surface_journeys = sequence_of(surface, "journeys");
        require(
            surface_journeys
                .map(|list| !list.is_empty())
                .unwrap_or(false),
            file,
            &format!("surface {target} journeys are required"),
        )?;
        for journey in surface_journeys.expect("checked") {
            let name = journey.as_str().unwrap_or_default();
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
            let command = string_of(runner, "command").filter(|value| !value.is_empty());
            let capability = string_of(runner, "capability").filter(|value| !value.is_empty());
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
            let args_are_strings = sequence_of(runner, "args")
                .map(|list| list.iter().all(Value::is_string))
                .unwrap_or(true);
            require(
                args_are_strings,
                file,
                &format!("surface {target} runner args must be strings"),
            )?;
        }
        for (index, override_entry) in sequence_of(surface, "journeyOverrides")
            .unwrap_or(&Vec::new())
            .iter()
            .enumerate()
        {
            require(
                override_entry.as_mapping().is_some(),
                file,
                &format!("surface {target} journeyOverrides.{index} must be an object"),
            )?;
            let when = map_of(override_entry, "when");
            require(
                when.map(|map| !map.is_empty()).unwrap_or(false),
                file,
                &format!("surface {target} journeyOverrides.{index}.when is required"),
            )?;
            let override_journeys = sequence_of(override_entry, "journeys");
            require(
                override_journeys
                    .map(|list| !list.is_empty())
                    .unwrap_or(false),
                file,
                &format!("surface {target} journeyOverrides.{index}.journeys are required"),
            )?;
            for (key, value) in when.expect("checked") {
                let name = key.as_str().unwrap_or_default();
                require(
                    !sensitive(name),
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
            for journey in override_journeys.expect("checked") {
                let name = journey.as_str().unwrap_or_default();
                require(
                    journeys.get(Value::from(name)).is_some(),
                    file,
                    &format!("surface {target} journey override {name} is unknown"),
                )?;
            }
        }
        if let Some(conditions) = map_of(surface, "conditions") {
            for name in key_names(conditions) {
                require(
                    !sensitive(&name),
                    file,
                    &format!("secret condition {name} must use secretRefs"),
                )?;
            }
        }
        if let Some(env) = map_of(surface, "env") {
            for (target_name, source_name) in env {
                require(
                    !target_name.as_str().unwrap_or_default().is_empty(),
                    file,
                    &format!("surface {target} env target is required"),
                )?;
                require(
                    !source_name.as_str().unwrap_or_default().is_empty(),
                    file,
                    &format!("surface {target} env source is required"),
                )?;
            }
        }
    }
    Ok(())
}
