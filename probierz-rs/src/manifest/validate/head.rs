use crate::manifest::*;


/// Judge one manifest document. Order follows the declaration itself:
/// identity, repositories, surfaces, journeys, then the policies that read
/// them.
pub fn validate(document: &Value, file: &Path) -> Answer {
    require(document.as_mapping().is_some(), file, "is not an object")?;
    require(
        document.get("schemaVersion").and_then(Value::as_u64) == Some(1),
        file,
        "schemaVersion must be 1",
    )?;
    let app_id = string_of(document, "appId").unwrap_or_default();
    require(!app_id.is_empty(), file, "appId is required")?;
    require(
        !string_of(document, "owner").unwrap_or_default().is_empty(),
        file,
        "owner is required",
    )?;
    let repositories = sequence_of(document, "repositories");
    require(
        repositories.map(|list| !list.is_empty()).unwrap_or(false),
        file,
        "repositories are required",
    )?;
    let surfaces = map_of(document, "surfaces");
    require(surfaces.is_some(), file, "surfaces are required")?;
    let journeys = map_of(document, "journeys");
    require(journeys.is_some(), file, "journeys are required")?;
    let surfaces = surfaces.expect("checked");
    let journeys = journeys.expect("checked");

    let first_use = journeys.get(Value::from("onboarding-first-use"));
    if first_use.is_some() {
        require(
            valid_id(string_of(document, "productId").unwrap_or_default()),
            file,
            "productId is required and must be stable for onboarding-first-use",
        )?;
        let retain = document
            .get("artifacts")
            .and_then(|artifacts| map_of(artifacts, "retain"));
        for name in ["pullRequestDays", "nightlyDays", "adhocDays"] {
            let days = retain
                .and_then(|map| map.get(Value::from(name)))
                .and_then(Value::as_f64);
            require(days.map(|value| value > 0.0).unwrap_or(false), file,
                &format!("artifacts.retain.{name} is required and must be positive for onboarding-first-use"))?;
        }
        let redact: Vec<String> = document
            .get("artifacts")
            .and_then(|artifacts| sequence_of(artifacts, "redact"))
            .map(|list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        require(
            !redact.is_empty(),
            file,
            "artifacts.redact must contain redaction keys for onboarding-first-use",
        )?;
        for name in ["TOKEN", "SECRET", "PASSWORD", "KEY", "COOKIE", "AUTH"] {
            require(
                redact.iter().any(|entry| entry == name),
                file,
                &format!("artifacts.redact must include {name} for onboarding-first-use"),
            )?;
        }
    }
    validate_repositories_and_surfaces(file, repositories.expect("checked"), surfaces, journeys)?;
    validate_journeys(document, file, surfaces, journeys)?;
    validate_policies(surfaces, document, file, journeys)?;
    Ok(())
}

