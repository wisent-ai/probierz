//! Everything the evaluation is given, read and checked before a browser is
//! opened: the approved brief, the rubric and the page address. A missing or
//! malformed field is refused by name here, so a run never reaches the model
//! with half its inputs.

use serde_json::{json, Map};

use super::constants::{SCHEMA_VERSION, WEIGHT_TOLERANCE};
use crate::specs::*;

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read JSON {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("cannot read JSON {}: {error}", path.display()))
}

pub(super) fn non_empty(value: &Value, field: &str) -> Result<String, String> {
    match value.as_str() {
        Some(text) if !text.trim().is_empty() => Ok(text.to_string()),
        _ => Err(format!("{field} is required")),
    }
}

pub(super) fn string_array(value: &Value, field: &str) -> Result<Vec<String>, String> {
    let items = value.as_array().filter(|items| !items.is_empty());
    let strings: Option<Vec<String>> = items.and_then(|items| {
        items
            .iter()
            .map(|item| {
                item.as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_string)
            })
            .collect()
    });
    strings.ok_or_else(|| format!("{field} must be a non-empty string array"))
}

fn object<'v>(value: &'v Value, message: &str) -> Result<&'v Map<String, Value>, String> {
    value.as_object().ok_or_else(|| message.to_string())
}

/// The approved brief at PROBIERZ_LANDING_BRIEF (relative to the harness).
pub(super) fn brief(context: &Context) -> Result<(PathBuf, Value), String> {
    let path = context.harness.join(context.required(
        "PROBIERZ_LANDING_BRIEF",
        "the approved landing brief, relative to the harness",
    )?);
    let raw = read_json(&path)?;
    if raw["schemaVersion"] != SCHEMA_VERSION {
        return Err(format!(
            "landing brief schemaVersion must be {SCHEMA_VERSION}"
        ));
    }
    let action = object(
        &raw["primaryAction"],
        "landing brief primaryAction.kind must be url, dialog, or form",
    )?;
    let kind = action
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(kind, "url" | "dialog" | "form") {
        return Err("landing brief primaryAction.kind must be url, dialog, or form".into());
    }
    let claims = raw["approvedClaims"]
        .as_array()
        .filter(|claims| !claims.is_empty())
        .ok_or("landing brief approvedClaims must contain substantiated claims")?
        .iter()
        .enumerate()
        .map(|(index, claim)| {
            object(claim, &format!("landing brief approvedClaims.{index} must be an object"))?;
            Ok(json!({
                "claim": non_empty(&claim["claim"], &format!("approvedClaims.{index}.claim"))?,
                "evidence": non_empty(&claim["evidence"], &format!("approvedClaims.{index}.evidence"))?,
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    object(&raw["brand"], "landing brief brand must be an object")?;
    let forbidden = raw["brand"]["forbidden"]
        .as_array()
        .filter(|items| {
            items
                .iter()
                .all(|item| item.as_str().is_some_and(|text| !text.trim().is_empty()))
        })
        .ok_or("landing brief brand.forbidden must be a string array")?;
    let mut brief = json!({
        "schemaVersion": SCHEMA_VERSION,
        "product": non_empty(&raw["product"], "product")?,
        "audience": non_empty(&raw["audience"], "audience")?,
        "problem": non_empty(&raw["problem"], "problem")?,
        "promise": non_empty(&raw["promise"], "promise")?,
        "primaryAction": {
            "label": non_empty(&raw["primaryAction"]["label"], "primaryAction.label")?,
            "kind": kind,
            "target": non_empty(&raw["primaryAction"]["target"], "primaryAction.target")?,
        },
        "approvedClaims": claims,
        "requiredProof": string_array(&raw["requiredProof"], "requiredProof")?,
        "brand": {
            "canonicalAssets": string_array(&raw["brand"]["canonicalAssets"], "brand.canonicalAssets")?,
            "rules": string_array(&raw["brand"]["rules"], "brand.rules")?,
            "forbidden": forbidden,
        },
        "analyticsOwner": non_empty(&raw["analyticsOwner"], "analyticsOwner")?,
    });
    if let Some(secondary) = raw.get("secondaryAction") {
        object(secondary, "landing brief secondaryAction must be an object")?;
        brief["secondaryAction"] = json!({
            "label": non_empty(&secondary["label"], "secondaryAction.label")?,
            "purpose": non_empty(&secondary["purpose"], "secondaryAction.purpose")?,
        });
    }
    if let Some(notes) = raw.get("notes") {
        if !notes
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Err("landing brief notes must be a string array".into());
        }
        brief["notes"] = notes.clone();
    }
    Ok((path, brief))
}

/// The rubric at apps/landing-page/rubric.json in the harness.
pub(super) fn rubric(context: &Context) -> Result<Value, String> {
    let raw = read_json(&context.harness.join("apps/landing-page/rubric.json"))?;
    let invalid = "landing rubric schemaVersion or dimensions are invalid";
    if raw["schemaVersion"] != SCHEMA_VERSION {
        return Err(invalid.into());
    }
    let name = non_empty(&raw["name"], "rubric.name")?;
    let overall = raw["overallMinimum"]
        .as_f64()
        .filter(|minimum| (0.0..=1.0).contains(minimum))
        .ok_or("landing rubric overallMinimum must be between 0 and 1")?;
    let mut dimensions = Map::new();
    let mut weight = 0.0;
    for (key, candidate) in object(&raw["dimensions"], invalid)? {
        object(candidate, &format!("rubric {key} must be an object"))?;
        let share = candidate["weight"]
            .as_f64()
            .filter(|share| *share > 0.0)
            .ok_or_else(|| format!("rubric {key}.weight must be positive"))?;
        let minimum = candidate["minimum"]
            .as_f64()
            .filter(|minimum| (0.0..=1.0).contains(minimum))
            .ok_or_else(|| format!("rubric {key}.minimum must be between 0 and 1"))?;
        weight += share;
        dimensions.insert(
            key.clone(),
            json!({
                "label": non_empty(&candidate["label"], &format!("rubric.{key}.label"))?,
                "weight": share,
                "minimum": minimum,
                "criterion": non_empty(&candidate["criterion"], &format!("rubric.{key}.criterion"))?,
            }),
        );
    }
    if (weight - 1.0).abs() > WEIGHT_TOLERANCE {
        return Err(format!("landing rubric weights total {weight}, expected 1"));
    }
    let mut gates = Map::new();
    for (key, value) in object(
        &raw["deterministicGates"],
        "landing rubric deterministicGates must be an object",
    )? {
        gates.insert(
            key.clone(),
            Value::String(non_empty(
                value,
                &format!("rubric.deterministicGates.{key}"),
            )?),
        );
    }
    Ok(json!({
        "schemaVersion": SCHEMA_VERSION,
        "name": name,
        "overallMinimum": overall,
        "dimensions": dimensions,
        "deterministicGates": gates,
        "modelInstructions": string_array(&raw["modelInstructions"], "rubric.modelInstructions")?,
        "recommendationPriorities": string_array(&raw["recommendationPriorities"], "rubric.recommendationPriorities")?,
    }))
}

/// The page under evaluation, from BASE_URL: HTTPS, or HTTP on loopback, no credentials.
pub(super) fn target(context: &Context) -> Result<String, String> {
    let raw = context.required("BASE_URL", "the landing page under evaluation")?;
    let url = url::Url::parse(&raw).map_err(|_| "BASE_URL must be an absolute URL".to_string())?;
    let loopback = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
    );
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("BASE_URL must use HTTPS or loopback HTTP".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("BASE_URL must not contain credentials".into());
    }
    Ok(url.to_string())
}
