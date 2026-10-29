use crate::apphooks::*;
use serde_json::json;

mod expect;
use expect::{capacity_sum, seeded, slack_field};

/// The organization the fixture state names. A state without one used to
/// read as "", so the writer PATCHed `org_id=eq.` - no row - and still
/// recorded the statement as written, and the verifier read empty tables.
fn fixture_org_id(state: &Value) -> Result<String, Failure> {
    state
        .get("orgId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            Failure::config(
                "apphook.oko.state",
                "the Oko fixture state names no orgId; run the fixture setup hook first",
            )
        })
}

/// A PostgREST answer's rows, or a failure naming the table whose answer
/// was not a list - read as no rows, it failed the checks for the wrong reason.
fn table_rows(answer: Value, table: &str) -> Result<Vec<Value>, Failure> {
    match answer {
        Value::Array(rows) => Ok(rows),
        other => Err(Failure::unavailable(
            "apphook.oko.verify",
            format!("the {table} answer is not a list of rows: {other}"),
        )),
    }
}
pub(crate) fn oko_writer_update(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    required_oko(source, false)?;
    let (path, mut state) = read_state(source)?;
    let run_id = configured(source, "PROBIERZ_RUN_ID")?;
    let org_id = fixture_org_id(&state)?;
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
    let run_id = configured(source, "PROBIERZ_RUN_ID")?;
    let decision = feedback_decision(run_id);
    let slack_state = state.get("slack").expect("checked");
    let feedback_source = json!({
        "org_id": fixture_org_id(&state)?,
        "channel_id": slack_field(slack_state, "channel")?,
        "message_ts": slack_field(slack_state, "replyTs")?,
        "thread_ts": slack_field(slack_state, "parentTs")?,
        "author_id": slack_field(slack_state, "replyAuthorId")?,
        "message_text": slack_field(slack_state, "replyText")?,
        "bot_context": slack_field(slack_state, "botContext")?
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
            // Pending and not yet attempted: the attempt count and the
            // last error are the table's declared defaults.
            "status": "pending"
        })),
    )?;
    let apply = || {
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
        )
    };
    apply()?;
    // The same decision applied again: one decision row afterwards proves
    // the RPC is idempotent.
    apply()?;
    let marker = match decision.pointer("/mutations/0/stringValue") {
        Some(marker) => marker.clone(),
        None => {
            return Err(Failure::config(
                "apphook.oko.feedback",
                format!("the feedback decision for run {run_id} states no first mutation stringValue to expect: {decision}"),
            ));
        }
    };
    state["feedback"]["applied"] = json!(true);
    state["feedback"]["appliedAt"] = json!(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
    state["strategy"]["expectedStatement"] = marker.clone();
    write_state(&path, &state)?;
    Ok(json!({
        "orgId": state["orgId"],
        "decisionId": state["feedback"]["decisionId"],
        "marker": marker,
        "reapplied": true
    }))
}

pub(crate) fn oko_verify_fixture(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    required_oko(source, false)?;
    let (_, state) = read_state(source)?;
    let org_id = fixture_org_id(&state)?;
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
    let contexts = table_rows(contexts, "company_context")?;
    let metrics = table_rows(metrics, "company_metrics")?;
    let initiatives = table_rows(initiatives, "company_initiatives")?;
    let decisions = table_rows(decisions, "company_decisions")?;
    let events = table_rows(events, "oko_strategic_feedback_events")?;
    let capacity = capacity_sum(&initiatives, "capacity_percent", "the company_initiatives row")?;
    let seeded = seeded(&state)?;
    let run_id = configured(source, "PROBIERZ_RUN_ID")?;
    let expected_statement = state
        .pointer("/strategy/expectedStatement")
        .or_else(|| state.pointer("/strategy/initial/northStar/statement"));
    let feedback_applied = state.pointer("/feedback/applied").and_then(Value::as_bool).ok_or_else(|| {
        Failure::config("apphook.oko.state", "the Oko fixture state says neither way whether feedback was applied")
    })?;
    let decision_id = state.pointer("/feedback/decisionId");
    let checks = json!({
        "context": matches!(contexts.as_slice(), [_]),
        "statement": contexts.first().and_then(|item| item.get("north_star_statement")) == expected_statement,
        "metrics": metrics.len() == seeded.metrics,
        "initiatives": initiatives.len() == seeded.initiatives,
        "capacity": capacity == seeded.capacity,
        "baselineDecision": decisions.iter().any(|item| item.get("id").and_then(Value::as_str) == Some(&deterministic_uuid(&["oko-e2e", run_id, "decision", "0"]))),
        "feedbackDecision": !feedback_applied || matches!(decisions.iter().filter(|item| item.get("id") == decision_id).collect::<Vec<_>>().as_slice(), [_]),
        "feedbackEvent": !feedback_applied || matches!(events.as_slice(), [event] if event.get("status").and_then(Value::as_str) == Some("processed"))
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
        let channel = slack_field(slack_state, "channel")?;
        if let Some(reply_ts) = slack_state.get("replyTs").and_then(Value::as_str) {
            slack(
                source,
                SLACK_USER,
                "chat.delete",
                json!({ "channel": channel, "ts": reply_ts }),
            )?;
        }
        if let Some(parent_ts) = slack_state.get("parentTs").and_then(Value::as_str) {
            slack(
                source,
                SLACK_BOT,
                "chat.delete",
                json!({ "channel": channel, "ts": parent_ts }),
            )?;
        }
    }
    let run_id = configured(source, "PROBIERZ_RUN_ID")?;
    let org_id = match &state {
        Some(state) => fixture_org_id(state)?,
        // No state file: setup ended before writing one, and the organization
        // it creates is the one this run's id names.
        None => deterministic_uuid(&["oko-e2e-org", run_id]),
    };
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
