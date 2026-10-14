use crate::evidence::*;
use serde_json::json;
pub fn retention(harness: &Path, app_id: Option<&str>, at: Option<&str>, apply: bool) -> Answer {
    let app_id = app_id
        .ok_or_else(|| Failure::invalid("evidence.retention", "retention needs an app ID"))?;
    let at = match at {
        Some(value) => DateTime::parse_from_rfc3339(value)
            .map_err(|_| Failure::invalid("evidence.retention", "invalid retention time"))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };
    let app = manifest::load(harness, app_id)?;
    let document = app.json()?;
    let mut items = Vec::new();
    for file in manifests_below(&harness.join("test-results").join(app_id))? {
        let run = crate::json_file::required(&file)?;
        // Every run records its kind and when it started; a manifest that
        // states neither is refused, never retained under a kind it was not.
        let unreadable = |what: &str| {
            Failure::invalid(
                "evidence.retention",
                format!("{} states no {what}", file.display()),
            )
        };
        let kind = run
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| unreadable("kind"))?;
        let expiry = expires_at(
            run.get("completedAt")
                .or_else(|| run.get("startedAt"))
                .and_then(Value::as_str)
                .ok_or_else(|| unreadable("completedAt or startedAt"))?,
            retention_days(&document, kind)?,
        )?;
        let expired = expiry
            .as_deref()
            .and_then(|expiry| DateTime::parse_from_rfc3339(expiry).ok())
            .is_some_and(|value| value <= at);
        items.push(json!({
            "type": "run", "appId": app_id, "runId": run.get("runId").cloned().unwrap_or(Value::Null),
            "kind": kind, "path": file.parent().unwrap_or(&file).to_string_lossy(), "expiresAt": expiry, "expired": expired,
        }));
    }
    let protected = harness.join("test-results").join(".protected").join(app_id);
    if protected.exists() {
        for file in files_below(&protected, false)?
            .into_iter()
            .filter(|file| file.extension().and_then(|value| value.to_str()) == Some("pev"))
        {
            let (header, _, _) = read_header(&file)?;
            let expiry = header
                .get("expiresAt")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let expired = DateTime::parse_from_rfc3339(expiry).is_ok_and(|value| value <= at);
            items.push(json!({
                "type": "protected", "appId": app_id, "runId": header.get("runId").cloned().unwrap_or(Value::Null),
                "kind": header.get("runKind").cloned().unwrap_or(Value::Null), "path": file.to_string_lossy(),
                "expiresAt": expiry, "expired": expired,
            }));
        }
    }
    items.sort_by(|left, right| {
        left.get("expiresAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(
                right
                    .get("expiresAt")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .then_with(|| {
                left.get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .cmp(
                        right
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
            })
    });
    let expired = items
        .iter()
        .filter(|item| item.get("expired").and_then(Value::as_bool) == Some(true))
        .count();
    // A removal that failed used to be listed as removed anyway; it is listed
    // under notRemoved with its error, and the command fails after answering.
    let mut removed = Vec::new();
    let mut not_removed = Vec::new();
    if apply {
        for item in items
            .iter()
            .filter(|item| item.get("expired").and_then(Value::as_bool) == Some(true))
        {
            let Some(text) = item.get("path").and_then(Value::as_str) else {
                not_removed
                    .push(json!({ "item": item, "error": "the expired item names no path" }));
                continue;
            };
            let path = PathBuf::from(text);
            let outcome = if item.get("type").and_then(Value::as_str) == Some("run") {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            match outcome {
                Ok(()) => removed.push(json!(path.to_string_lossy())),
                Err(error) => not_removed
                    .push(json!({ "path": path.to_string_lossy(), "error": error.to_string() })),
            }
        }
    }
    let at_text = at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let action = if apply {
        "retention.apply"
    } else {
        "retention.plan"
    };
    let written = audit_access(
        harness,
        action,
        "allowed",
        Some(app_id),
        None,
        None,
        json!({ "expired": expired, "removed": removed.len(), "notRemoved": not_removed.len(), "at": at_text }),
    );
    print_json(&json!({
        "schemaVersion": 1, "appId": app_id, "at": at_text, "expired": expired, "items": items,
        "applied": apply, "removed": removed, "notRemoved": not_removed,
    }))?;
    allowed_audit_written(action, written)?;
    if !not_removed.is_empty() {
        return Err(Failure::new(
            "evidence.retention",
            crate::failure::Code::Unknown,
            format!(
                "{} expired evidence path(s) could not be removed; notRemoved names each with its error",
                not_removed.len()
            ),
        ));
    }
    Ok(())
}
