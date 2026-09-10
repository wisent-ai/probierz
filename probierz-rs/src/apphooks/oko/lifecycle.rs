use serde_json::json;
use crate::apphooks::*;
pub(crate) fn oko_seed(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    if !requires_oko_fixture(source) {
        return Ok(json!({ "skipped": "autonomy journey uses isolated local fixtures" }));
    }
    required_oko(source, requires_slack(source))?;
    let scoped = scoped_oko_source(source)?;
    let account = ensure_technical_account(&scoped)?;
    let account_id = account
        .get("userId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let run_id = source.get("PROBIERZ_RUN_ID").expect("required");
    let run_hash = hash12(run_id);
    let org_id = deterministic_uuid(&["oko-e2e-org", run_id]);
    let org_slug = format!("{ORG_PREFIX}{run_hash}");
    let seed_slack = requires_slack(source);
    let mut parent_ts: Option<String> = None;
    let mut reply_ts: Option<String> = None;
    let result = (|| {
        delete_organization(&scoped, &org_id)?;
        supabase(
            &scoped,
            "/rest/v1/organizations",
            "POST",
            Some("return=minimal"),
            Some(json!({
                "id": org_id,
                "slug": org_slug,
                "name": format!("Oko E2E {run_hash}")
            })),
        )?;
        supabase(
            &scoped,
            "/rest/v1/organization_members",
            "POST",
            Some("return=minimal"),
            Some(json!({
                "org_id": org_id,
                "user_id": account_id,
                "role": "owner"
            })),
        )?;
        supabase(
            &scoped,
            "/rest/v1/company_context",
            "POST",
            Some("resolution=merge-duplicates,return=minimal"),
            Some(json!({
                "org_id": org_id,
                "schema_version": 2,
                "north_star_statement": "initializing"
            })),
        )?;
        supabase(
            &scoped,
            "/rest/v1/rpc/oko_apply_company_strategy",
            "POST",
            None,
            Some(json!({
                "p_org_id": org_id,
                "p_document": strategy_document(run_id)
            })),
        )?;

        let slack_state = if seed_slack {
            let channel = source.get("OKO_E2E_SLACK_CHANNEL").expect("required");
            let bot_context = format!("Oko strategy review for isolated run {run_id}.");
            let reply_text = format!(
                "Decision: accept the deterministic correction for {run_id}. Set the north-star statement to \"Probierz Slack feedback applied [{run_id}]\" because every release needs traceable product evidence."
            );
            let parent = slack(
                source.get("OKO_E2E_SLACK_BOT_TOKEN").expect("required"),
                "chat.postMessage",
                json!({ "channel": channel, "text": bot_context }),
                &[],
            )?;
            let posted_parent = parent
                .get("ts")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            parent_ts = Some(posted_parent.clone());
            let reply = slack(
                source.get("OKO_E2E_SLACK_USER_TOKEN").expect("required"),
                "chat.postMessage",
                json!({ "channel": channel, "thread_ts": posted_parent, "text": reply_text }),
                &[],
            )?;
            let posted_reply = reply
                .get("ts")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            reply_ts = Some(posted_reply.clone());
            supabase(
                &scoped,
                "/rest/v1/oko_slack_thread_watches",
                "POST",
                Some("resolution=merge-duplicates,return=minimal"),
                Some(json!({
                    "org_id": org_id,
                    "channel_id": channel,
                    "thread_ts": posted_parent,
                    "bot_context": bot_context,
                    "last_reply_ts": posted_parent,
                    "last_scanned_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
                })),
            )?;
            let author = reply
                .pointer("/message/user")
                .or_else(|| reply.get("user"))
                .and_then(Value::as_str)
                .unwrap_or("oko-e2e-user");
            json!({
                "channel": channel,
                "parentTs": posted_parent,
                "replyTs": posted_reply,
                "replyAuthorId": author,
                "botContext": bot_context,
                "replyText": reply_text
            })
        } else {
            Value::Null
        };
        let path = state_path(source);
        let state = json!({
            "schemaVersion": 2,
            "runId": run_id,
            "orgId": org_id,
            "orgSlug": org_slug,
            "userId": account_id,
            "emailHash": account.get("emailHash").cloned().unwrap_or(Value::Null),
            "strategy": { "initial": strategy_document(run_id) },
            "feedback": {
                "decisionId": deterministic_uuid(&["oko-e2e-feedback-decision", run_id]),
                "applied": false
            },
            "slack": slack_state
        });
        write_state(&path, &state)?;
        Ok(path)
    })();
    match result {
        Ok(path) => Ok(json!({
            "orgId": org_id,
            "orgSlug": org_slug,
            "accountCreated": account.get("created").and_then(Value::as_bool).unwrap_or(false),
            "stateFile": path,
            "env": { "OKO_E2E_EMAIL": scoped.get("OKO_E2E_EMAIL").expect("scoped") }
        })),
        Err(error) => {
            // Every rollback is best effort, matching Promise.allSettled in the former hook.
            let channel = source
                .get("OKO_E2E_SLACK_CHANNEL")
                .map(String::as_str)
                .unwrap_or_default();
            if let Some(reply_ts) = reply_ts {
                let _ = slack(
                    source
                        .get("OKO_E2E_SLACK_USER_TOKEN")
                        .map(String::as_str)
                        .unwrap_or_default(),
                    "chat.delete",
                    json!({ "channel": channel, "ts": reply_ts }),
                    &["message_not_found"],
                );
            }
            if let Some(parent_ts) = parent_ts {
                let _ = slack(
                    source
                        .get("OKO_E2E_SLACK_BOT_TOKEN")
                        .map(String::as_str)
                        .unwrap_or_default(),
                    "chat.delete",
                    json!({ "channel": channel, "ts": parent_ts }),
                    &["message_not_found"],
                );
            }
            let _ = delete_organization(&scoped, &org_id);
            let _ = supabase(
                &scoped,
                &format!("/auth/v1/admin/users/{account_id}"),
                "DELETE",
                None,
                None,
            );
            Err(error)
        }
    }
}

