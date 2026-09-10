use serde_json::json;
use crate::apphooks::*;
pub(crate) fn oko_writer_update(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    required_oko(source, false)?;
    let (path, mut state) = read_state(source)?;
    let run_id = source.get("PROBIERZ_RUN_ID").expect("required");
    let org_id = state
        .get("orgId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let marker = format!("Probierz writer R+1 [{run_id}]");
    supabase(
        source,
        &format!("/rest/v1/company_context?org_id=eq.{org_id}"),
        "PATCH",
        Some("return=minimal"),
        Some(json!({
            "north_star_statement": marker,
            "updated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
        })),
    )?;
    state["strategy"]["expectedStatement"] = json!(marker);
    state["writer"] = json!({ "marker": marker, "advancedAt": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true) });
    write_state(&path, &state)?;
    Ok(json!({ "orgId": org_id, "marker": marker }))
}

pub(crate) fn oko_apply_feedback(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    required_oko(source, false)?;
    let (path, mut state) = read_state(source)?;
    if state.get("slack").is_none_or(Value::is_null) {
        return Err(Failure::config(
            "apphook.oko.feedback",
            "Slack feedback fixture was not selected for this run",
        ));
    }
    let run_id = source.get("PROBIERZ_RUN_ID").expect("required");
    let decision = feedback_decision(run_id);
    let slack_state = state.get("slack").expect("checked");
    let feedback_source = json!({
        "org_id": state.get("orgId").cloned().unwrap_or(Value::Null),
        "channel_id": slack_state.get("channel").cloned().unwrap_or(Value::Null),
        "message_ts": slack_state.get("replyTs").cloned().unwrap_or(Value::Null),
        "thread_ts": slack_state.get("parentTs").cloned().unwrap_or(Value::Null),
        "author_id": slack_state.get("replyAuthorId").cloned().unwrap_or(Value::Null),
        "message_text": slack_state.get("replyText").cloned().unwrap_or(Value::Null),
        "bot_context": slack_state.get("botContext").cloned().unwrap_or(Value::Null)
    });
    supabase(
        source,
        "/rest/v1/oko_strategic_feedback_events",
        "POST",
        Some("resolution=merge-duplicates,return=minimal"),
        Some(json!({
            "id": deterministic_uuid(&["oko-e2e-feedback-event", run_id]),
            "org_id": feedback_source["org_id"],
            "channel_id": feedback_source["channel_id"],
            "message_ts": feedback_source["message_ts"],
            "thread_ts": feedback_source["thread_ts"],
            "author_id": feedback_source["author_id"],
            "message_text": feedback_source["message_text"],
            "bot_context": feedback_source["bot_context"],
            "status": "pending",
            "attempt_count": 1,
            "last_attempt_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            "last_error": ""
        })),
    )?;
    for _ in 0..2 {
        supabase(
            source,
            "/rest/v1/rpc/oko_apply_strategic_feedback",
            "POST",
            None,
            Some(json!({
                "p_source": feedback_source,
                "p_decision": decision,
                "p_decision_id": state["feedback"]["decisionId"]
            })),
        )?;
    }
    let marker = decision
        .pointer("/mutations/0/stringValue")
        .cloned()
        .unwrap_or(Value::Null);
    state["feedback"]["applied"] = json!(true);
    state["feedback"]["appliedAt"] = json!(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
    state["strategy"]["expectedStatement"] = marker.clone();
    write_state(&path, &state)?;
    Ok(json!({
        "orgId": state["orgId"],
        "decisionId": state["feedback"]["decisionId"],
        "marker": marker,
        "duplicateAttempts": 2
    }))
}

pub(crate) fn oko_verify_fixture(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    required_oko(source, false)?;
    let (_, state) = read_state(source)?;
    let org_id = state
        .get("orgId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let contexts = supabase(
        source,
        &format!("/rest/v1/company_context?select=north_star_statement&org_id=eq.{org_id}"),
        "GET",
        None,
        None,
    )?;
    let metrics = supabase(
        source,
        &format!("/rest/v1/company_metrics?select=id&org_id=eq.{org_id}"),
        "GET",
        None,
        None,
    )?;
    let initiatives = supabase(
        source,
        &format!("/rest/v1/company_initiatives?select=id,capacity_percent&org_id=eq.{org_id}"),
        "GET",
        None,
        None,
    )?;
    let decisions = supabase(
        source,
        &format!("/rest/v1/company_decisions?select=id,decision&org_id=eq.{org_id}"),
        "GET",
        None,
        None,
    )?;
    let events = if let Some(reply_ts) = state.pointer("/slack/replyTs").and_then(Value::as_str) {
        supabase(source, &format!("/rest/v1/oko_strategic_feedback_events?select=status,attempt_count&org_id=eq.{org_id}&message_ts=eq.{reply_ts}"), "GET", None, None)?
    } else {
        json!([])
    };
    let contexts = contexts.as_array().cloned().unwrap_or_default();
    let metrics = metrics.as_array().cloned().unwrap_or_default();
    let initiatives = initiatives.as_array().cloned().unwrap_or_default();
    let decisions = decisions.as_array().cloned().unwrap_or_default();
    let events = events.as_array().cloned().unwrap_or_default();
    let capacity: f64 = initiatives
        .iter()
        .map(|item| {
            item.get("capacity_percent")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
        })
        .sum();
    let run_id = source.get("PROBIERZ_RUN_ID").expect("required");
    let expected_statement = state
        .pointer("/strategy/expectedStatement")
        .or_else(|| state.pointer("/strategy/initial/northStar/statement"));
    let feedback_applied = state
        .pointer("/feedback/applied")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let decision_id = state.pointer("/feedback/decisionId");
    let checks = json!({
        "context": contexts.len() == 1,
        "statement": contexts.first().and_then(|item| item.get("north_star_statement")) == expected_statement,
        "metrics": metrics.len() == 4,
        "initiatives": initiatives.len() == 13,
        "capacity": (capacity - 100.0).abs() < 0.0001,
        "baselineDecision": decisions.iter().any(|item| item.get("id").and_then(Value::as_str) == Some(&deterministic_uuid(&["oko-e2e", run_id, "decision", "0"]))),
        "feedbackDecision": !feedback_applied || decisions.iter().filter(|item| item.get("id") == decision_id).count() == 1,
        "feedbackEvent": !feedback_applied || (events.len() == 1 && events[0].get("status").and_then(Value::as_str) == Some("processed"))
    });
    let failed: Vec<&str> = checks
        .as_object()
        .expect("object")
        .iter()
        .filter_map(|(name, value)| (value.as_bool() != Some(true)).then_some(name.as_str()))
        .collect();
    if !failed.is_empty() {
        return Err(Failure::new(
            "apphook.oko.verify",
            Code::Refused,
            format!("Oko fixture verification failed: {}", failed.join(", ")),
        ));
    }
    Ok(
        json!({ "orgId": org_id, "checks": checks, "capacity": capacity, "decisionCount": decisions.len() }),
    )
}

pub(crate) fn oko_cleanup(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    if !requires_oko_fixture(source) {
        return Ok(json!({ "skipped": "autonomy journey uses isolated local fixtures" }));
    }
    required_oko(source, false)?;
    let path = state_path(source);
    let state: Option<Value> = if path.exists() {
        Some(serde_json::from_slice(&fs::read(&path)?)?)
    } else {
        None
    };
    if state
        .as_ref()
        .and_then(|state| state.get("slack"))
        .is_some_and(|value| !value.is_null())
    {
        required_oko(source, true)?;
    }
    if let Some(slack_state) = state
        .as_ref()
        .and_then(|state| state.get("slack"))
        .filter(|value| !value.is_null())
    {
        let channel = slack_state
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(reply_ts) = slack_state.get("replyTs").and_then(Value::as_str) {
            slack(
                source.get("OKO_E2E_SLACK_USER_TOKEN").expect("required"),
                "chat.delete",
                json!({ "channel": channel, "ts": reply_ts }),
                &["message_not_found"],
            )?;
        }
        if let Some(parent_ts) = slack_state.get("parentTs").and_then(Value::as_str) {
            slack(
                source.get("OKO_E2E_SLACK_BOT_TOKEN").expect("required"),
                "chat.delete",
                json!({ "channel": channel, "ts": parent_ts }),
                &["message_not_found"],
            )?;
        }
    }
    let run_id = source.get("PROBIERZ_RUN_ID").expect("required");
    let org_id = state
        .as_ref()
        .and_then(|state| state.get("orgId"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| deterministic_uuid(&["oko-e2e-org", run_id]));
    delete_organization(source, &org_id)?;
    let user_id = state
        .as_ref()
        .and_then(|state| state.get("userId"))
        .and_then(Value::as_str);
    if let Some(user_id) = user_id {
        supabase(
            source,
            &format!("/auth/v1/admin/users/{user_id}"),
            "DELETE",
            None,
            None,
        )?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(json!({ "deletedOrganization": org_id, "deletedAccount": user_id.is_some() }))
}

