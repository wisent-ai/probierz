use serde_json::json;
use crate::stado::*;

pub(crate) fn manifest_string<'a>(document: &'a serde_yaml::Value, path: &[&str]) -> Option<&'a str> {
    let mut current = document;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str()
}

pub(crate) fn remote_secret_env(harness: &Path, app_id: &str, names: &[&str]) -> Result<Value, Failure> {
    let application = manifest::load(harness, app_id)?;
    let configured = application
        .document
        .get("secretRefs")
        .and_then(serde_yaml::Value::as_mapping);
    let mut answer = Map::new();
    for name in names {
        let (reference, item, field) = match *name {
            "STADO_MODEL_ROUTER_TOKEN" => {
                (MODEL_ROUTER_REFERENCE, "probierz-model-router", "token")
            }
            "PROBIERZ_MODEL_AGENT_SECRET" => (
                MODEL_AGENT_REFERENCE,
                "probierz-agent-auth",
                "agent_auth_secret",
            ),
            "PROBIERZ_SEO_RECEIPT_PRIVATE_KEY" => (
                SEO_KEY_REFERENCE,
                "probierz-seo-receipt-signing",
                "private_key",
            ),
            _ => continue,
        };
        let found = configured
            .and_then(|mapping| mapping.get(serde_yaml::Value::from(*name)))
            .and_then(serde_yaml::Value::as_str);
        let Some(found) = found else {
            continue;
        };
        if found != reference {
            return Err(Failure::config(
                "stado.submit",
                format!("Remote runs require {reference} for {name}."),
            ));
        }
        answer.insert((*name).to_string(), json!({ "item": item, "field": field }));
    }
    Ok(Value::Object(answer))
}

pub(crate) fn setup_step_count(target: &str) -> Result<u64, Failure> {
    match target {
        "web" | "electron" | "mobile:ios" | "mobile:android" | "desktop:win" | "desktop:cua" => Ok(2),
        "desktop:mac" => Ok(3),
        "tui" => Ok(1),
        _ => Err(Failure::config("stado.submit", format!("unknown target: {target} (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:cua|desktop:win|tui)"))),
    }
}

pub(crate) fn provisioning_budget(target: &str, provision: Option<&Provision>) -> Result<u64, Failure> {
    let setup = if target == "tui" {
        0
    } else {
        setup_step_count(target)?
    };
    let source_build = matches!(provision, Some(Provision::CargoRelease { .. })) as u64
        + (target == "desktop:cua"
            && matches!(provision, Some(Provision::AppBundle { app_id, .. }) if app_id == "stado"))
            as u64;
    Ok((1 + setup + source_build) * SETUP_STEP_TIMEOUT_MS)
}

pub(crate) fn selected_run_budget(
    harness: &Path,
    app_id: &str,
    target: &str,
    environment: &[(String, String)],
    provision: Option<&Provision>,
) -> Result<u64, Failure> {
    let application = manifest::load(harness, app_id)?;
    let surface = application
        .document
        .get("surfaces")
        .and_then(|surfaces| surfaces.get(target))
        .ok_or_else(|| {
            Failure::config(
                "stado.submit",
                format!("app {app_id} has no {target} surface"),
            )
        })?;
    let mut selected = BTreeMap::new();
    if let Some(conditions) = surface
        .get("conditions")
        .and_then(serde_yaml::Value::as_mapping)
    {
        for (name, value) in conditions {
            if let (Some(name), Some(value)) = (name.as_str(), yaml_scalar(value)) {
                selected.insert(name.to_string(), value);
            }
        }
    }
    for (name, value) in environment {
        selected.insert(name.clone(), value.clone());
    }
    if let Some(bindings) = surface.get("env").and_then(serde_yaml::Value::as_mapping) {
        for (target_name, source_name) in bindings {
            let (Some(target_name), Some(source_name)) =
                (target_name.as_str(), source_name.as_str())
            else {
                continue;
            };
            if let Some(value) = selected
                .get(source_name)
                .cloned()
                .or_else(|| std::env::var(source_name).ok())
            {
                selected.insert(source_name.to_string(), value.clone());
                selected.insert(target_name.to_string(), value);
            }
        }
    }
    let journeys = manifest::surface_journeys(surface, &selected);
    let mut budget = 0_u64;
    for journey in journeys {
        budget = budget.saturating_add(
            application
                .document
                .get("journeys")
                .and_then(|all| all.get(&journey))
                .and_then(|value| value.get("timeoutMs"))
                .and_then(serde_yaml::Value::as_u64)
                .unwrap_or(0),
        );
    }
    Ok(budget.saturating_add(provisioning_budget(target, provision)?))
}

pub(crate) fn conservative_watch_budget(harness: &Path, app_id: &str) -> Result<u64, Failure> {
    let application = manifest::load(harness, app_id)?;
    let journey_budget = application
        .document
        .get("journeys")
        .and_then(serde_yaml::Value::as_mapping)
        .map(|journeys| {
            journeys
                .values()
                .filter_map(|journey| journey.get("timeoutMs").and_then(serde_yaml::Value::as_u64))
                .sum()
        })
        .unwrap_or(0_u64);
    let surfaces = application
        .document
        .get("surfaces")
        .and_then(serde_yaml::Value::as_mapping);
    let mut provision = 0_u64;
    if let Some(surfaces) = surfaces {
        for target in surfaces.keys().filter_map(serde_yaml::Value::as_str) {
            provision = provision.max(provisioning_budget(
                target,
                Some(&Provision::CargoRelease {
                    app_id: app_id.to_string(),
                    binary: app_id.to_string(),
                    manifest_path: "Cargo.toml".to_string(),
                }),
            )?);
        }
    }
    Ok(journey_budget.saturating_add(provision))
}

pub(crate) fn yaml_scalar(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(text) => Some(text.clone()),
        serde_yaml::Value::Number(number) => Some(number.to_string()),
        serde_yaml::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

