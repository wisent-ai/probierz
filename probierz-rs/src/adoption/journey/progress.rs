use serde_json::json;
use crate::adoption::*;
pub(crate) fn progress_file() -> Result<PathBuf, Failure> {
    let state_root = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| PathBuf::from(value.trim()))
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|home| home.join(".local/state"))
        })
        .ok_or_else(|| fail("onboarding.state", "home directory is unavailable"))?;
    Ok(state_root.join("probierz/onboarding.json"))
}

pub(crate) fn read_progress() -> Option<Value> {
    let file = progress_file().ok()?;
    serde_json::from_slice(&fs::read(file).ok()?).ok()
}

pub(crate) fn write_progress(progress: &Value) -> Result<(), Failure> {
    let file = progress_file()?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = format!("{}\n", serde_json::to_string_pretty(progress)?);
    write_private(&file, body.as_bytes())?;
    Ok(())
}

pub(crate) fn write_initial_progress(definition: &Value) -> Result<(), Failure> {
    write_progress(&json!({
        "product_id": definition["product_id"],
        "journey_id": definition["journey_id"],
        "journey_version": definition["journey_version"],
        "status": "in_progress",
        "evidence": {},
        "started_at": now_iso(),
    }))
}

pub(crate) fn ordered_screens(definition: &Value) -> Vec<&Value> {
    let Some(screens) = definition.get("screens").and_then(Value::as_array) else {
        return Vec::new();
    };
    let by_id: HashMap<&str, &Value> = screens
        .iter()
        .filter_map(|screen| {
            screen
                .get("screen_id")
                .and_then(Value::as_str)
                .map(|id| (id, screen))
        })
        .collect();
    let mut ordered = Vec::new();
    let mut current = definition
        .get("entry_screen_id")
        .and_then(Value::as_str)
        .and_then(|id| by_id.get(id).copied());
    while let Some(screen) = current {
        if ordered
            .iter()
            .any(|seen: &&Value| std::ptr::eq(*seen, screen))
        {
            break;
        }
        ordered.push(screen);
        current = screen
            .get("transitions")
            .and_then(Value::as_array)
            .and_then(|transitions| {
                transitions.iter().min_by_key(|transition| {
                    transition
                        .get("priority")
                        .and_then(Value::as_i64)
                        .unwrap_or(i64::MAX)
                })
            })
            .and_then(|transition| transition.get("next_screen_id"))
            .and_then(Value::as_str)
            .and_then(|id| by_id.get(id).copied());
    }
    ordered
}

pub(crate) fn onboarding_definition() -> Value {
    json!({
        "schema_version": 1,
        "product_id": "probierz",
        "journey_id": "first-use",
        "journey_version": "2026-09-03.2",
        "entry_screen_id": "adopt-existing-project",
        "first_success_fact": "passing_quality_evidence_written",
        "published_at": "2026-09-03T00:00:00Z",
        "source_revision": "probierz-first-use-2026-09-03.2",
        "screens": [
            {
                "screen_id": "adopt-existing-project",
                "screen_kind": "import",
                "title_key": "probierz.first_use.adopt.title",
                "body_key": "probierz.first_use.adopt.body",
                "required": false,
                "actions": ["import", "skip"],
                "transitions": [{
                    "next_screen_id": "choose-one-journey",
                    "reason_code": "project_adopted_or_skipped",
                    "priority": 10
                }],
                "presentation": {
                    "title": "Bring your existing Probierz project",
                    "body": "Choose another Probierz repository to adopt its validated apps/<appId>/probierz.yaml manifests and established package spec directories. Probierz preserves the definitions, reports every conflict, and does not run a journey. Skip keeps this project empty and usable."
                }
            },
            {
                "screen_id": "choose-one-journey",
                "screen_kind": "explanation",
                "title_key": "probierz.first_use.choose.title",
                "body_key": "probierz.first_use.choose.body",
                "required": true,
                "actions": ["advance"],
                "transitions": [{
                    "next_screen_id": "read-the-evidence",
                    "reason_code": "journey_selected",
                    "priority": 10
                }],
                "presentation": {
                    "title": "Start with one declared journey",
                    "body": "Probierz runs evidence for a product, surface and user journey declared in an application manifest. Begin with `probierz apps`, then inspect one registration with `probierz app APP_ID`; its surface names the target and spec you can run instead of guessing either."
                }
            },
            {
                "screen_id": "read-the-evidence",
                "screen_kind": "explanation",
                "title_key": "probierz.first_use.evidence.title",
                "body_key": "probierz.first_use.evidence.body",
                "required": true,
                "actions": ["advance"],
                "transitions": [{
                    "next_screen_id": "receipts-follow-runs",
                    "reason_code": "evidence_model_explained",
                    "priority": 10
                }],
                "presentation": {
                    "title": "A completed run leaves quality evidence",
                    "body": "The first durable result is a run manifest, not a claim that a suite passed. Probierz binds the report, analysis, source and build identities, conditions and artifact hashes into that record, then reports a pass or fail without averaging failures away."
                }
            },
            {
                "screen_id": "receipts-follow-runs",
                "screen_kind": "explanation",
                "title_key": "probierz.first_use.receipts.title",
                "body_key": "probierz.first_use.receipts.body",
                "required": true,
                "actions": ["advance"],
                "transitions": [{
                    "next_screen_id": "produce-evidence",
                    "reason_code": "receipt_sequence_explained",
                    "priority": 10
                }],
                "presentation": {
                    "title": "Release receipts come after recorded runs",
                    "body": "A release gate consumes exact run IDs and identities. Once the required journeys have qualifying evidence, `probierz receipt` signs the resulting verdict for a release; it cannot replace the underlying run records or turn missing evidence green."
                }
            },
            {
                "screen_id": "produce-evidence",
                "screen_kind": "guided_query",
                "title_key": "probierz.first_use.run.title",
                "body_key": "probierz.first_use.run.body",
                "required": true,
                "completion_evidence": {
                    "kind": "fact",
                    "fact": "passing_quality_evidence_written",
                    "operator": "eq",
                    "value": true
                },
                "actions": ["run"],
                "transitions": [],
                "presentation": {
                    "title": "Produce your first evidence record",
                    "body": "Run one declared spec on its target with `probierz run TARGET --app APP_ID --spec SPEC`. When the command succeeds and Probierz writes a passing evidence block into the run manifest, this journey is complete. A failed run remains honest, actionable evidence, but the release gate stays red and this first-success step stays open.",
                    "command": "probierz run TARGET --app APP_ID --spec SPEC",
                    "result": "A passing JSON run result with its run ID, evidence checks and run-manifest path"
                }
            }
        ],
        "analytics_contract": {
            "contract_version": "1",
            "surface": "cli_first_use",
            "exposure_event": "onboarding_step_viewed",
            "primary_action_event": "onboarding_step_completed",
            "completion_event": "onboarding_completed",
            "first_success_event": "onboarding_first_success_observed"
        }
    })
}
