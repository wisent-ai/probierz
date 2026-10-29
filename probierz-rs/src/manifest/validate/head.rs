use crate::manifest::*;

/// Judge one manifest document. Order follows the declaration itself:
/// identity, repositories, surfaces, journeys, then the policies that read
/// them.
pub fn validate(document: &Value, file: &Path) -> Answer {
    require(document.as_mapping().is_some(), file, "is not an object")?;
    let schema = document.get("schemaVersion");
    require(
        schema.and_then(Value::as_u64) == Some(MANIFEST_SCHEMA_VERSION),
        file,
        &format!(
            "schemaVersion must be {MANIFEST_SCHEMA_VERSION}, the one app manifest schema this Probierz reads; it is {}",
            schema.map_or_else(|| "absent".to_string(), |value| format!("{value:?}"))
        ),
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
        // The retention periods are a schema: every field `Retention` names
        // is required; that each is positive is checked for every manifest
        // with the other policies.
        let retain = document
            .get("artifacts")
            .and_then(|artifacts| artifacts.get("retain"));
        match retain.cloned().map(serde_yaml::from_value::<Retention>) {
            Some(Ok(_)) => {}
            Some(Err(error)) => require(
                false,
                file,
                &format!("artifacts.retain is incomplete for onboarding-first-use: {error}"),
            )?,
            None => require(
                false,
                file,
                "artifacts.retain is required for onboarding-first-use",
            )?,
        }
        require(
            document
                .get("artifacts")
                .and_then(|artifacts| sequence_of(artifacts, "redact"))
                .is_some_and(|list| !list.is_empty()),
            file,
            "artifacts.redact must name what onboarding-first-use records may not publish",
        )?;
    }
    let redaction = Redaction::of_document(document);
    validate_repositories_and_surfaces(
        file,
        repositories.expect("checked"),
        surfaces,
        journeys,
        &redaction,
    )?;
    validate_journeys(document, file, surfaces, journeys)?;
    validate_policies(surfaces, document, file, journeys)?;
    Ok(())
}

/// The retention periods a first-use manifest must state, in days; read
/// only to require that every one is there.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct Retention {
    pull_request_days: f64,
    nightly_days: f64,
    adhoc_days: f64,
}
