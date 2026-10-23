use crate::apphooks::*;
use serde_json::json;

/// The text field `key` of an answer Oko's seed depends on, refused naming
/// the answer and the field when it is absent: an id the seed cannot read is
/// one it could neither use nor roll back.
fn answered_text(answer: &Value, key: &str, what: &str) -> Result<String, Failure> {
    answer
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            Failure::unavailable(
                "apphook.oko.seed",
                format!("{what} answered without a text {key}: {answer}"),
            )
        })
}

pub(crate) fn oko_seed(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    required_oko(source, requires_slack(source))?;
    let scoped = scoped_oko_source(source)?;
    let account = ensure_technical_account(&scoped)?;
    let account_id = answered_text(&account, "userId", "the technical account")?;
    let created = account
        .get("created")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            Failure::unavailable(
                "apphook.oko.seed",
                format!(
                    "the technical account answered without saying whether it was created: {account}"
                ),
            )
        })?;
    let run_id = source.get("PROBIERZ_RUN_ID").expect("required");
    let run_hash = hash12(run_id);
    let org_id = deterministic_uuid(&["oko-e2e-org", run_id]);
    let org_slug = format!("{ORG_PREFIX}{run_hash}");
    let seed_slack = requires_slack(source);
    let mut parent_ts: Option<String> = None;
    let mut reply_ts: Option<String> = None;
    let result = (|| -> Result<PathBuf, Failure> {
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
                source,
                SLACK_BOT,
                "chat.post_message",
                json!({ "channel": channel, "text": bot_context }),
            )?;
            let posted_parent = answered_text(&parent, "ts", "Slack's post of the thread parent")?;
            parent_ts = Some(posted_parent.clone());
            let reply = slack(
                source,
                SLACK_USER,
                "chat.post_message",
                json!({ "channel": channel, "thread_ts": posted_parent, "text": reply_text }),
            )?;
            let posted_reply = answered_text(&reply, "ts", "Slack's post of the thread reply")?;
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
            let author = answered_text(&reply, "user", "Slack's post of the thread reply")?;
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
            "runId": run_id,
            "orgId": org_id,
            "orgSlug": org_slug,
            "userId": account_id,
            "emailHash": account.get("emailHash"),
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
            "accountCreated": created,
            "stateFile": path,
            "env": { "OKO_E2E_EMAIL": scoped.get("OKO_E2E_EMAIL").expect("scoped") }
        })),
        Err(mut error) => {
            let mut rollback_errors = Vec::new();
            // A Slack post exists only when the channel was read to make it, so
            // the channel is present whenever there is a post to delete.
            let channel = source.get("OKO_E2E_SLACK_CHANNEL");
            if let (Some(reply_ts), Some(channel)) = (&reply_ts, channel) {
                if let Err(failure) = slack(
                    source,
                    SLACK_USER,
                    "chat.delete",
                    json!({ "channel": channel, "ts": reply_ts }),
                ) {
                    rollback_errors.push(format!("Slack reply {reply_ts} in {channel}: {failure}"));
                }
            }
            if let (Some(parent_ts), Some(channel)) = (&parent_ts, channel) {
                if let Err(failure) = slack(
                    source,
                    SLACK_BOT,
                    "chat.delete",
                    json!({ "channel": channel, "ts": parent_ts }),
                ) {
                    rollback_errors
                        .push(format!("Slack thread {parent_ts} in {channel}: {failure}"));
                }
            }
            if let Err(failure) = delete_organization(&scoped, &org_id) {
                rollback_errors.push(format!("organization {org_id}: {failure}"));
            }
            if let Err(failure) = supabase(
                &scoped,
                &format!("/auth/v1/admin/users/{account_id}"),
                "DELETE",
                None,
                None,
            ) {
                rollback_errors.push(format!("account {account_id}: {failure}"));
            }
            if !rollback_errors.is_empty() {
                error
                    .detail
                    .push_str("; rollback could not confirm removal: ");
                error.detail.push_str(&rollback_errors.join("; "));
            }
            Err(error)
        }
    }
}
