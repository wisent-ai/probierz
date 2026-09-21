//! Retention over the evidence a fleet run leaves in Stado's object store.
//!
//! `retention` walks this harness's own `test-results` tree. The evidence of
//! a run dispatched to the fleet does not live there: it is written into the
//! `probierz` object namespace on the host that ran it, and nothing has ever
//! expired it. On charless-mac-mini on 2026-09-21 that store held 34.9 GiB —
//! the single largest occupant of a host that was 15.4 GiB below its disk
//! target and therefore refused as a release builder for darwin-arm64.
//!
//! What is deleted is decided by the application's own manifest, the same
//! `retention_days` the local plan uses, because the product that writes the
//! evidence declares how long it is kept. A plan is printed unless `--apply`
//! is given, and an object whose age the store cannot state is reported and
//! never removed.

use serde_json::json;

use crate::evidence::*;

/// The prefix a fleet run's evidence is written under.
const RESULTS_ROOT: &str = "stado://probierz/results/";
/// The kind a fleet run's evidence is retained as. The objects under
/// `results/` are the archives and logs of dispatched runs, written flat and
/// not attributed to an application, so they are kept for the adhoc window —
/// the one an application's manifest declares, or the product's own default
/// when no application is named.
const DEFAULT_KIND: &str = "adhoc";
/// What `retention_days` falls back to when a manifest declares nothing.
const DEFAULT_RETENTION_DAYS: f64 = 14.0;
const SECONDS_PER_DAY: i64 = 86_400;

/// One object in the fleet store, as retention sees it.
struct FleetObject {
    key: String,
    uri: String,
    bytes: i64,
    modified: Option<DateTime<Utc>>,
}

fn read_objects() -> Result<Vec<FleetObject>, Failure> {
    let listed = list_objects(RESULTS_ROOT.trim_end_matches(char::from(47)))?;
    let mut objects = Vec::new();
    for item in listed {
        let key = item
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Failure::unavailable("evidence.fleet_retention", "a listed object has no key")
            })?
            .to_string();
        let uri = item
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Failure::unavailable("evidence.fleet_retention", "a listed object has no uri")
            })?
            .to_string();
        let bytes = item
            .get("size")
            .and_then(Value::as_i64)
            .or_else(|| item.get("bytes").and_then(Value::as_i64))
            .unwrap_or_default();
        let modified = ["modified", "modified_at", "updated_at", "last_modified"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|value| value.with_timezone(&Utc));
        objects.push(FleetObject {
            key,
            uri,
            bytes,
            modified,
        });
    }
    Ok(objects)
}

pub fn fleet_retention(harness: &Path, app_id: Option<&str>, at: Option<&str>, apply: bool) -> Answer {
    let at = match at {
        Some(value) => DateTime::parse_from_rfc3339(value)
            .map_err(|_| Failure::invalid("evidence.fleet_retention", "invalid retention time"))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };
    let days = match app_id {
        Some(app) => {
            let application = manifest::load(harness, app)?;
            retention_days(&yaml_json(&application.document)?, DEFAULT_KIND)?
        }
        None => DEFAULT_RETENTION_DAYS,
    };
    let objects = read_objects()?;
    let mut items = Vec::new();
    let mut expired_bytes = 0i64;
    let mut kept_bytes = 0i64;
    let mut undated = 0i64;
    let mut removed = 0i64;
    for object in &objects {
        let expired = match object.modified {
            Some(modified) if days > 0.0 => {
                (at - modified).num_seconds() as f64 > days * SECONDS_PER_DAY as f64
            }
            _ => false,
        };
        if object.modified.is_none() {
            undated += 1;
        }
        if expired {
            expired_bytes += object.bytes;
        } else {
            kept_bytes += object.bytes;
        }
        let mut record = json!({
            "key": object.key,
            "bytes": object.bytes,
            "expired": expired,
            "retentionDays": days,
            "modifiedAt": object.modified.map(|value| value.to_rfc3339()),
        });
        if expired && apply {
            remove_object(&object.uri)?;
            removed += 1;
            record["removed"] = Value::Bool(true);
        }
        items.push(record);
    }
    print_json(&json!({
        "schemaVersion": 1,
        "root": RESULTS_ROOT,
        "appId": app_id,
        "at": at.to_rfc3339(),
        "objects": items.len(),
        "undatedObjects": undated,
        "expiredBytes": expired_bytes,
        "keptBytes": kept_bytes,
        "removed": removed,
        "applied": apply,
        "items": items,
    }))
}
