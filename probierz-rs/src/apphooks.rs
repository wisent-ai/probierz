//! Product-specific setup and evaluation hooks executed by Probierz itself.
//!
//! Manifests name these capabilities. They are not script paths: the same
//! implementation serves lifecycle runs and the `probierz apphook` command.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use base64::Engine;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use crate::failure::{print_json, write_private, Answer, Code, Failure};

const OKO_REQUIRED: [&str; 5] = [
    "PROBIERZ_RUN_ID",
    "OKO_E2E_EMAIL",
    "OKO_E2E_SUPABASE_URL",
    "OKO_E2E_SUPABASE_ANON_KEY",
    "OKO_E2E_SUPABASE_SERVICE_ROLE_KEY",
];
const OKO_SLACK_REQUIRED: [&str; 3] = [
    "OKO_E2E_SLACK_BOT_TOKEN",
    "OKO_E2E_SLACK_USER_TOKEN",
    "OKO_E2E_SLACK_CHANNEL",
];
const OKO_ACCOUNT_REQUIRED: [&str; 3] = [
    "OKO_E2E_EMAIL",
    "OKO_E2E_SUPABASE_URL",
    "OKO_E2E_SUPABASE_SERVICE_ROLE_KEY",
];
const OKO_BROKER_REQUIRED: [&str; 3] = [
    "OKO_E2E_EMAIL",
    "OKO_E2E_OTP_BROKER_URL",
    "OKO_E2E_OTP_BROKER_TOKEN",
];
const ORG_PREFIX: &str = "probierz-oko-e2e-";

/// What an application manifest may name, and what each capability accepts.
///
/// A manifest declares a capability by name, so an operator has to be able to
/// read the same list the dispatcher matches on. `apphook --help` prints this.
#[macro_export]
macro_rules! apphook_help {
    () => {
        "\
Capabilities (named by an application manifest):
  oko.seed                            Create the isolated organization, author
                                      and fixture state one Oko journey needs
  oko.cleanup                         Remove the organization and account that
                                      seeding created
  oko.ensure-technical-account        Create or confirm the technical account
                                      the journey signs in as
  oko.wait-for-otp [OPTIONS]          Wait for the next one-time code and print
                                      JSON containing it for the journey
    --after <ISO>                     Ignore codes delivered before this instant
    --timeout-ms <MS>                 Give up after this long (default 90000)
  oko.writer-update                   Apply the writer update the journey expects
  oko.apply-feedback                  Apply the editorial feedback fixture
  oko.verify-fixture                  Confirm the seeded state is intact
  game-asset-creator.fixtures         Materialize the asset fixtures the suite reads
  game-asset-creator.visual-eval [OPTIONS]
                                      Score rendered assets against the rubric
                                      through the authenticated model router
    --models <DIR>                    Directory containing the GLBs to grade
    --out <DIR>                       Directory that receives renders and report
    --config <FILE>                   Pipeline configuration to read
    --rubric <NAME>                   Built-in rubric name or literal rubric
    --threshold <N>                   Score below which the evaluation fails

Environment:
  GAC_ROOT                            game_asset_creator repository; defaults to
                                      its root in the application manifest
  GAC_FIXTURE_DIR                     Fixture directory; defaults inside the
                                      current run artifacts or harness"
    };
}

pub fn supports(name: &str) -> bool {
    matches!(
        name,
        "oko.seed"
            | "oko.cleanup"
            | "oko.ensure-technical-account"
            | "oko.wait-for-otp"
            | "oko.writer-update"
            | "oko.apply-feedback"
            | "oko.verify-fixture"
            | "game-asset-creator.fixtures"
            | "game-asset-creator.visual-eval"
    )
}

pub fn execute(
    harness: &Path,
    capability: &str,
    args: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    match capability {
        "oko.seed" => oko_seed(environment),
        "oko.cleanup" => oko_cleanup(environment),
        "oko.ensure-technical-account" => ensure_technical_account(environment),
        "oko.wait-for-otp" => {
            let (after, timeout) = otp_options(args)?;
            oko_wait_for_otp(environment, after, timeout).map(|code| json!({ "code": code }))
        }
        "oko.writer-update" => oko_writer_update(environment),
        "oko.apply-feedback" => oko_apply_feedback(environment),
        "oko.verify-fixture" => oko_verify_fixture(environment),
        "game-asset-creator.fixtures" => gac_fixtures(harness, environment),
        "game-asset-creator.visual-eval" => gac_visual_eval(harness, args, environment),
        other => Err(Failure::invalid(
            "apphook",
            format!("unknown application capability: {other}"),
        )),
    }
}

pub fn command(harness: &Path, capability: &str, args: &[String]) -> Answer {
    let environment = std::env::vars().collect();
    let result = execute(harness, capability, args, &environment)?;
    print_json(&result)
}

fn required<'a>(
    source: &'a BTreeMap<String, String>,
    names: &[&str],
    message: &str,
) -> Result<&'a BTreeMap<String, String>, Failure> {
    let missing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| source.get(*name).is_none_or(String::is_empty))
        .collect();
    if missing.is_empty() {
        Ok(source)
    } else {
        Err(Failure::config(
            "apphook.environment",
            format!("{message}: {}", missing.join(", ")),
        ))
    }
}

fn selected_journeys(source: &BTreeMap<String, String>) -> BTreeSet<&str> {
    source
        .get("PROBIERZ_JOURNEYS")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect()
}

fn requires_slack(source: &BTreeMap<String, String>) -> bool {
    let journeys = selected_journeys(source);
    journeys.is_empty() || journeys.contains("slack-feedback")
}

fn requires_oko_fixture(source: &BTreeMap<String, String>) -> bool {
    let journeys = selected_journeys(source);
    journeys.is_empty()
        || journeys
            .iter()
            .any(|journey| *journey != "autonomy-experimental")
}

fn required_oko(source: &BTreeMap<String, String>, include_slack: bool) -> Result<(), Failure> {
    let mut names = OKO_REQUIRED.to_vec();
    if include_slack {
        names.extend(OKO_SLACK_REQUIRED);
    }
    required(source, &names, "missing Oko seed configuration").map(|_| ())
}

fn hash12(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))[..12].to_string()
}

fn deterministic_uuid(parts: &[&str]) -> String {
    let mut digest = Sha256::digest(parts.join(":").as_bytes());
    digest[6] = (digest[6] & 0x0f) | 0x40;
    digest[8] = (digest[8] & 0x3f) | 0x80;
    let value = hex::encode(&digest[..16]);
    format!(
        "{}-{}-{}-{}-{}",
        &value[..8],
        &value[8..12],
        &value[12..16],
        &value[16..20],
        &value[20..]
    )
}

fn fixture_slug(run_id: &str, kind: &str, index: Option<usize>) -> String {
    match index {
        Some(index) => format!("e2e-{}-{kind}-{}", hash12(run_id), index + 1),
        None => format!("e2e-{}-{kind}", hash12(run_id)),
    }
}

fn strategy_document(run_id: &str) -> Value {
    let names = ["Activation", "Retention", "Revenue", "Reliability"];
    let metrics: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            json!({
                "id": deterministic_uuid(&["oko-e2e", run_id, "metric", &index.to_string()]),
                "slug": fixture_slug(run_id, "metric", Some(index)),
                "name": name,
                "description": format!("Probierz metric {}", index + 1),
                "owner": "Oko E2E",
                "horizon": "quarterly",
                "unit": "percent",
                "baselineValue": 0,
                "targetValue": 100,
                "currentValue": 25 * (index + 1),
                "source": format!("probierz:{run_id}"),
                "status": "measured"
            })
        })
        .collect();
    let pillar_names = ["Research", "Product", "Distribution", "Operations"];
    let pillars: Vec<Value> = pillar_names
        .iter()
        .enumerate()
        .map(|(index, title)| {
            json!({
                "id": deterministic_uuid(&["oko-e2e", run_id, "pillar", &index.to_string()]),
                "slug": fixture_slug(run_id, "pillar", Some(index)),
                "title": title,
                "owner": "Oko E2E",
                "role": format!("Probierz pillar {}", index + 1),
                "successCriteria": [format!("Metric {} is measured", index + 1)],
                "linkedProductSlugs": [fixture_slug(run_id, "product", Some(index))]
            })
        })
        .collect();
    let initiative_slugs: Vec<String> = (0..13)
        .map(|index| fixture_slug(run_id, "initiative", Some(index)))
        .collect();
    let product_names = ["Oko", "Platform", "Research", "Distribution"];
    let products: Vec<Value> = product_names
        .iter()
        .enumerate()
        .map(|(index, title)| {
            json!({
                "id": deterministic_uuid(&["oko-e2e", run_id, "product", &index.to_string()]),
                "slug": fixture_slug(run_id, "product", Some(index)),
                "title": title,
                "owner": "Oko E2E",
                "role": format!("Probierz product {}", index + 1),
                "linkedPillarSlugs": [fixture_slug(run_id, "pillar", Some(index))],
                "activeInitiativeSlugs": initiative_slugs.iter().enumerate().filter_map(|(candidate, slug)| (candidate % 4 == index).then_some(slug)).collect::<Vec<_>>(),
                "blockers": [],
                "customerRelevance": "Deterministic E2E evidence",
                "researchRelevance": "Deterministic E2E evidence"
            })
        })
        .collect();
    let initiatives: Vec<Value> = initiative_slugs
        .iter()
        .enumerate()
        .map(|(index, slug)| {
            json!({
                "id": deterministic_uuid(&["oko-e2e", run_id, "initiative", &index.to_string()]),
                "slug": slug,
                "title": format!("Probierz initiative {}", index + 1),
                "owner": "Oko E2E",
                "status": "in_progress",
                "successMetric": names[index % names.len()],
                "linkedProductSlugs": [fixture_slug(run_id, "product", Some(index % 4))],
                "linkedPillarSlugs": [fixture_slug(run_id, "pillar", Some(index % 4))],
                "activeConversationSources": [format!("slack:{run_id}")],
                "artifactSlugs": [fixture_slug(run_id, "run-receipt", None)],
                "nextActions": [format!("Complete deterministic step {}", index + 1)],
                "targetDate": "2026-12-31",
                "budgetUSD": 1000 + index,
                "dependencySlugs": if index == 0 { Vec::<String>::new() } else { vec![initiative_slugs[index - 1].clone()] },
                "outcomeMetricSlugs": [fixture_slug(run_id, "metric", Some(index % names.len()))],
                "priority": 100 - index,
                "capacityPercent": if index == 12 { 10.0 } else { 7.5 },
                "planningStatus": "approved"
            })
        })
        .collect();
    json!({
        "schemaVersion": 2,
        "northStar": {
            "statement": format!("Probierz Oko reference strategy [{run_id}]"),
            "marketThesis": "Deterministic product evidence is a release requirement.",
            "whyWisentWins": "The product connects strategy, conversations, and execution.",
            "mustBecomeTrue": ["Every critical journey has current evidence."],
            "nonCriticalWork": ["Unseeded cosmetic variation."],
            "metrics": metrics
        },
        "pillars": pillars,
        "products": products,
        "initiatives": initiatives,
        "artifacts": [{
            "id": deterministic_uuid(&["oko-e2e", run_id, "artifact", "0"]),
            "slug": fixture_slug(run_id, "run-receipt", None),
            "title": "Probierz run receipt",
            "kind": "evidence",
            "owner": "Oko E2E",
            "location": format!("probierz:{run_id}"),
            "status": "active",
            "supportsPillarSlugs": pillars.iter().filter_map(|item| item.get("slug")).cloned().collect::<Vec<_>>(),
            "supportsProductSlugs": products.iter().filter_map(|item| item.get("slug")).cloned().collect::<Vec<_>>(),
            "supportsInitiativeSlugs": initiative_slugs
        }],
        "decisions": [{
            "id": deterministic_uuid(&["oko-e2e", run_id, "decision", "0"]),
            "decidedOn": "2026-07-13",
            "title": "Require deterministic Oko evidence",
            "decision": "Release only with current Probierz receipts.",
            "rationale": format!("Seeded by {run_id}"),
            "owner": "Oko E2E",
            "affectedProductSlugs": [fixture_slug(run_id, "product", Some(0))],
            "affectedInitiativeSlugs": [fixture_slug(run_id, "initiative", Some(0))],
            "reversibility": "reversible"
        }]
    })
}

fn feedback_decision(run_id: &str) -> Value {
    json!({
        "isDecision": true,
        "title": format!("Accept Probierz feedback [{run_id}]"),
        "decision": format!("The deterministic Slack correction for {run_id} is accepted."),
        "rationale": "The correction is explicit, scoped to the synthetic organization, and reversible.",
        "affectedProductSlugs": [fixture_slug(run_id, "product", Some(0))],
        "affectedInitiativeSlugs": [fixture_slug(run_id, "initiative", Some(0))],
        "reversibility": "reversible",
        "mutations": [{
            "entity": "north_star",
            "slug": "",
            "field": "statement",
            "stringValue": format!("Probierz Slack feedback applied [{run_id}]")
        }]
    })
}

fn technical_email(email: &str) -> Result<String, Failure> {
    let email = email.to_ascii_lowercase();
    let local = email.split('@').next().unwrap_or_default();
    if !local.contains("e2e") && !local.contains("probierz") {
        return Err(Failure::config(
            "apphook.oko.email",
            "OKO_E2E_EMAIL must be a dedicated address containing 'e2e' or 'probierz'",
        ));
    }
    Ok(email)
}

fn scoped_oko_source(
    source: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, Failure> {
    let email = source
        .get("OKO_E2E_EMAIL")
        .map(String::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut pieces = email.split('@');
    let address = pieces.next().unwrap_or_default();
    let domain = pieces.next().unwrap_or_default();
    if address.is_empty() || domain.is_empty() {
        return Err(Failure::config(
            "apphook.oko.email",
            "OKO_E2E_EMAIL must be a valid technical email address",
        ));
    }
    let base = address.split('+').next().unwrap_or(address);
    let run_id = source
        .get("PROBIERZ_RUN_ID")
        .map(String::as_str)
        .unwrap_or_default();
    let mut scoped = source.clone();
    scoped.insert(
        "OKO_E2E_EMAIL".into(),
        format!("{base}+probierz-{}@{domain}", hash12(run_id)),
    );
    Ok(scoped)
}

fn response_json(response: ureq::Response) -> Result<Value, Failure> {
    let status = response.status();
    let text = response
        .into_string()
        .map_err(|error| Failure::unavailable("apphook.http", error.to_string()))?;
    let body = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|_| json!({ "message": text }))
    };
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        let detail = body
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| body.get("error").and_then(Value::as_str))
            .or_else(|| body.pointer("/error/message").and_then(Value::as_str))
            .or_else(|| body.get("hint").and_then(Value::as_str))
            .unwrap_or("request failed");
        Err(Failure::new(
            "apphook.http",
            Code::Refused,
            format!("HTTP {status}: {detail}"),
        ))
    }
}

fn request_json(
    method: &str,
    endpoint: &str,
    headers: &[(&str, String)],
    body: Option<Value>,
) -> Result<Value, Failure> {
    let mut request = ureq::request(method, endpoint);
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let response = match body {
        Some(body) => request.send_json(body),
        None => request.call(),
    };
    match response {
        Ok(response) => response_json(response),
        Err(ureq::Error::Status(_, response)) => response_json(response),
        Err(error) => Err(Failure::unavailable("apphook.http", error.to_string())),
    }
}

fn supabase_headers(
    source: &BTreeMap<String, String>,
    prefer: Option<&str>,
) -> Vec<(&'static str, String)> {
    let key = source
        .get("OKO_E2E_SUPABASE_SERVICE_ROLE_KEY")
        .cloned()
        .unwrap_or_default();
    let mut headers = vec![
        ("apikey", key.clone()),
        ("Authorization", format!("Bearer {key}")),
        ("Content-Type", "application/json".into()),
    ];
    if let Some(prefer) = prefer {
        headers.push(("Prefer", prefer.into()));
    }
    headers
}

fn supabase(
    source: &BTreeMap<String, String>,
    path: &str,
    method: &str,
    prefer: Option<&str>,
    body: Option<Value>,
) -> Result<Value, Failure> {
    let base = source
        .get("OKO_E2E_SUPABASE_URL")
        .map(|value| value.trim_end_matches('/'))
        .unwrap_or_default();
    request_json(
        method,
        &format!("{base}{path}"),
        &supabase_headers(source, prefer),
        body,
    )
}

fn slack(
    token: &str,
    method: &str,
    payload: Value,
    accepted_errors: &[&str],
) -> Result<Value, Failure> {
    let body = request_json(
        "POST",
        &format!("https://slack.com/api/{method}"),
        &[
            ("Authorization", format!("Bearer {token}")),
            ("Content-Type", "application/json; charset=utf-8".into()),
        ],
        Some(payload),
    )?;
    if body.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("request failed");
        if !accepted_errors.contains(&error) {
            return Err(Failure::new(
                "apphook.oko.slack",
                Code::Refused,
                format!("Slack {method}: {error}"),
            ));
        }
    }
    Ok(body)
}

fn ensure_technical_account(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    required(source, &OKO_ACCOUNT_REQUIRED, "missing E2E configuration")?;
    let email = technical_email(
        source
            .get("OKO_E2E_EMAIL")
            .map(String::as_str)
            .unwrap_or_default(),
    )?;
    let base = source
        .get("OKO_E2E_SUPABASE_URL")
        .expect("required")
        .trim_end_matches('/');
    let headers = supabase_headers(source, None);
    let page = request_json(
        "GET",
        &format!("{base}/auth/v1/admin/users?page=1&per_page=1000"),
        &headers,
        None,
    )?;
    if let Some(existing) = page
        .get("users")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|user| {
            user.get("email")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case(&email))
        })
    {
        return Ok(json!({
            "created": false,
            "userId": existing.get("id").cloned().unwrap_or(Value::Null),
            "emailHash": hash12(&email)
        }));
    }
    let created = request_json(
        "POST",
        &format!("{base}/auth/v1/admin/users"),
        &headers,
        Some(json!({
            "email": email,
            "email_confirm": true,
            "user_metadata": { "purpose": "probierz-e2e" },
            "app_metadata": { "provider": "email", "providers": ["email"], "purpose": "probierz-e2e" }
        })),
    )?;
    let Some(user_id) = created.get("id").and_then(Value::as_str) else {
        return Err(Failure::new(
            "apphook.oko.account",
            Code::Refused,
            "Supabase did not return a created user ID",
        ));
    };
    Ok(json!({ "created": true, "userId": user_id, "emailHash": hash12(&email) }))
}

fn generate_admin_otp(source: &BTreeMap<String, String>) -> Result<String, Failure> {
    required(source, &OKO_ACCOUNT_REQUIRED, "missing E2E configuration")?;
    let email = technical_email(
        source
            .get("OKO_E2E_EMAIL")
            .map(String::as_str)
            .unwrap_or_default(),
    )?;
    let base = source
        .get("OKO_E2E_SUPABASE_URL")
        .expect("required")
        .trim_end_matches('/');
    let body = request_json(
        "POST",
        &format!("{base}/auth/v1/admin/generate_link"),
        &supabase_headers(source, None),
        Some(json!({ "type": "magiclink", "email": email })),
    )?;
    validated_otp(
        body.pointer("/properties/email_otp")
            .or_else(|| body.get("email_otp")),
        "Supabase Admin API",
    )
}

fn validated_otp(value: Option<&Value>, source_name: &str) -> Result<String, Failure> {
    let code = value
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_u64().map(|value| value.to_string()))
        })
        .unwrap_or_default();
    let code = code.trim();
    if !(6..=8).contains(&code.len()) || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Failure::new(
            "apphook.oko.otp",
            Code::Refused,
            format!("{source_name} returned an OTP outside the supported 6-8 digit range"),
        ));
    }
    Ok(code.to_string())
}

fn oko_wait_for_otp(
    source: &BTreeMap<String, String>,
    after: DateTime<Utc>,
    timeout: Duration,
) -> Result<String, Failure> {
    let broker_url = source
        .get("OKO_E2E_OTP_BROKER_URL")
        .filter(|value| !value.is_empty());
    let broker_token = source
        .get("OKO_E2E_OTP_BROKER_TOKEN")
        .filter(|value| !value.is_empty());
    if broker_url.is_none() && broker_token.is_none() {
        return generate_admin_otp(source);
    }
    required(source, &OKO_BROKER_REQUIRED, "missing E2E configuration")?;
    let email = technical_email(
        source
            .get("OKO_E2E_EMAIL")
            .map(String::as_str)
            .unwrap_or_default(),
    )?;
    let base = broker_url.expect("required").trim_end_matches('/');
    let mut endpoint = Url::parse(&format!("{base}/v1/otp"))
        .map_err(|error| Failure::config("apphook.oko.otp", error.to_string()))?;
    {
        let mut query = endpoint.query_pairs_mut();
        query.append_pair("email", &email);
        query.append_pair("after", &after.to_rfc3339_opts(SecondsFormat::Millis, true));
        if let Some(run_id) = source
            .get("PROBIERZ_RUN_ID")
            .filter(|value| !value.is_empty())
        {
            query.append_pair("runId", run_id);
        }
    }
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let body = request_json(
            "GET",
            endpoint.as_str(),
            &[(
                "Authorization",
                format!("Bearer {}", broker_token.expect("required")),
            )],
            None,
        )?;
        if let Some(code) = body.get("code") {
            return validated_otp(Some(code), "OTP broker");
        }
        thread::sleep(Duration::from_millis(1500));
    }
    Err(Failure::new(
        "apphook.oko.otp",
        Code::Refused,
        format!("OTP broker timed out after {}ms", timeout.as_millis()),
    ))
}

fn otp_options(args: &[String]) -> Result<(DateTime<Utc>, Duration), Failure> {
    let mut after = DateTime::<Utc>::from(SystemTime::now() - Duration::from_secs(30));
    let mut timeout_ms = 90_000_u64;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--after" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    Failure::invalid("apphook.oko.otp", "--after needs an ISO timestamp")
                })?;
                after = DateTime::parse_from_rfc3339(value)
                    .map_err(|_| {
                        Failure::invalid("apphook.oko.otp", "--after needs an ISO timestamp")
                    })?
                    .with_timezone(&Utc);
                index += 2;
            }
            "--timeout-ms" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    Failure::invalid("apphook.oko.otp", "--timeout-ms needs a positive integer")
                })?;
                timeout_ms = value
                    .parse::<u64>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        Failure::invalid("apphook.oko.otp", "--timeout-ms needs a positive integer")
                    })?;
                index += 2;
            }
            other => {
                return Err(Failure::invalid(
                    "apphook.oko.otp",
                    format!("unknown OTP option: {other}"),
                ))
            }
        }
    }
    Ok((after, Duration::from_millis(timeout_ms)))
}

fn state_path(source: &BTreeMap<String, String>) -> PathBuf {
    let root = source
        .get("PROBIERZ_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("test-results/oko-local"));
    root.join("diagnostics/oko-seed-state.json")
}

fn write_state(path: &Path, state: &Value) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(state)?;
    write_private(path, &bytes)?;
    Ok(())
}

fn read_state(source: &BTreeMap<String, String>) -> Result<(PathBuf, Value), Failure> {
    let path = state_path(source);
    if !path.exists() {
        return Err(Failure::config(
            "apphook.oko.state",
            format!("Oko seed state not found: {}", path.display()),
        ));
    }
    let state: Value = serde_json::from_slice(&fs::read(&path)?)?;
    let expected = source
        .get("PROBIERZ_RUN_ID")
        .map(String::as_str)
        .unwrap_or_default();
    if state.get("runId").and_then(Value::as_str) != Some(expected) {
        return Err(Failure::config(
            "apphook.oko.state",
            format!(
                "Oko seed state belongs to {}, not {expected}",
                state.get("runId").and_then(Value::as_str).unwrap_or("")
            ),
        ));
    }
    Ok((path, state))
}

fn delete_organization(source: &BTreeMap<String, String>, org_id: &str) -> Result<(), Failure> {
    supabase(
        source,
        &format!("/rest/v1/organizations?id=eq.{org_id}"),
        "DELETE",
        Some("return=minimal"),
        None,
    )?;
    Ok(())
}

fn oko_seed(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
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

fn oko_writer_update(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
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

fn oko_apply_feedback(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
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

fn oko_verify_fixture(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
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

fn oko_cleanup(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
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

fn glb(model: Value) -> Result<Vec<u8>, Failure> {
    let mut source = serde_json::to_string(&model)?;
    while source.len() % 4 != 0 {
        source.push(' ');
    }
    let length = 12 + 8 + source.len();
    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&(length as u32).to_le_bytes());
    bytes.extend_from_slice(&(source.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"JSON");
    bytes.extend_from_slice(source.as_bytes());
    Ok(bytes)
}

fn model_json(triangles: usize) -> Value {
    json!({
        "asset": { "version": "2.0" },
        "accessors": [{ "count": triangles * 3 }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "mode": 4 }] }],
        "materials": [{}]
    })
}

fn gac_fixtures(harness: &Path, source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    let directory = source
        .get("GAC_FIXTURE_DIR")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            source
                .get("PROBIERZ_ARTIFACTS")
                .filter(|value| !value.trim().is_empty())
                .map(|root| Path::new(root).join("fixtures/game-asset-creator"))
        })
        .unwrap_or_else(|| harness.join("test-results/game-asset-creator/fixtures"));
    fs::create_dir_all(&directory)?;
    let valid = directory.join("valid-6k.glb");
    let over_budget = directory.join("over-budget.glb");
    let corrupt = directory.join("corrupt.glb");
    let fake_skarbiec = directory.join("skarbiec");
    let config = directory.join("pipeline.config.json");
    fs::write(&valid, glb(model_json(6000))?)?;
    fs::write(&over_budget, glb(model_json(99_999))?)?;
    fs::write(&corrupt, b"definitely not a glb file")?;
    fs::write(
        &fake_skarbiec,
        b"#!/bin/sh\nif [ \"$1\" = \"get\" ]; then\n  case \"$2\" in\n    TEXT2GAME_ACCOUNT) echo '{\"fields\":{\"login_email\":\"fixture@example.com\",\"login_password\":\"fixture\"}}' ;;\n    BRAMA) echo '{\"fields\":{\"agent_auth_secret\":\"fixture-brama-key\"}}' ;;\n    *) echo \"item not found: $2\" >&2; exit 1 ;;\n  esac\n  exit 0\nfi\necho \"unknown command: $1\" >&2\nexit 1\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_skarbiec, fs::Permissions::from_mode(0o755))?;
    }
    let document = json!({
        "browser": { "headless": true },
        "credentials": {
            "username": "skarbiec://TEXT2GAME_ACCOUNT/login_email",
            "password": "skarbiec://TEXT2GAME_ACCOUNT/login_password"
        },
        "models": { "brama": {
            "url": "https://model-router.example",
            "key": "skarbiec://BRAMA/agent_auth_secret",
            "model": "any"
        }},
        "studio": {
            "loginUrl": "https://studio.example/login",
            "generateUrl": "https://studio.example/generate",
            "selectors": {
                "loginUser": "#u",
                "loginPassword": "#p",
                "loginSubmit": "#go",
                "promptInput": "#prompt",
                "generateSubmit": "#gen"
            },
            "artifact": { "pollExpression": "null", "timeoutMs": 1000, "intervalMs": 100 }
        },
        "verify": { "enabled": true, "triTarget": 6000, "triTolerancePct": 100 }
    });
    write_private(&config, &serde_json::to_vec_pretty(&document)?)?;
    Ok(json!({
        "dir": directory,
        "valid": valid,
        "overBudget": over_budget,
        "corrupt": corrupt,
        "fakeSkarbiec": fake_skarbiec,
        "config": config,
        "env": { "GAC_FIXTURE_DIR": directory }
    }))
}

#[derive(Default)]
struct VisualOptions {
    models: Option<String>,
    out: Option<String>,
    config: Option<String>,
    rubric: Option<String>,
    threshold: Option<String>,
}

fn visual_options(args: &[String]) -> Result<VisualOptions, Failure> {
    let mut options = VisualOptions::default();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].strip_prefix("--").ok_or_else(|| {
            Failure::invalid(
                "apphook.gac.visual-eval",
                format!("unexpected argument: {}", args[index]),
            )
        })?;
        let value = args
            .get(index + 1)
            .ok_or_else(|| {
                Failure::invalid("apphook.gac.visual-eval", format!("--{name} needs a value"))
            })?
            .clone();
        match name {
            "models" => options.models = Some(value),
            "out" => options.out = Some(value),
            "config" => options.config = Some(value),
            "rubric" => options.rubric = Some(value),
            "threshold" => options.threshold = Some(value),
            other => {
                return Err(Failure::invalid(
                    "apphook.gac.visual-eval",
                    format!("unknown visual-eval option: --{other}"),
                ))
            }
        }
        index += 2;
    }
    Ok(options)
}

fn resolve_skarbiec(value: &mut Value, binary: &str) -> Result<(), Failure> {
    match value {
        Value::String(text) if text.starts_with("skarbiec://") => {
            let reference = text.trim_start_matches("skarbiec://");
            let (item, field) = reference.split_once('/').ok_or_else(|| {
                Failure::config(
                    "apphook.gac.config",
                    format!("invalid Skarbiec reference: {text}"),
                )
            })?;
            let output = Command::new(binary)
                .args(["get", item])
                .output()
                .map_err(|error| {
                    Failure::new(
                        "apphook.gac.skarbiec",
                        Code::Prerequisite,
                        format!("failed to run {binary}: {error}"),
                    )
                })?;
            if !output.status.success() {
                return Err(Failure::new(
                    "apphook.gac.skarbiec",
                    Code::Refused,
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ));
            }
            let document: Value = serde_json::from_slice(&output.stdout)?;
            let resolved = document
                .pointer(&format!("/fields/{field}"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Failure::config(
                        "apphook.gac.skarbiec",
                        format!("Skarbiec item {item} has no field {field}"),
                    )
                })?;
            *text = resolved.to_string();
        }
        Value::Array(values) => {
            for value in values {
                resolve_skarbiec(value, binary)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                resolve_skarbiec(value, binary)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn load_gac_config(path: &Path, environment: &BTreeMap<String, String>) -> Result<Value, Failure> {
    let mut value: Value = serde_json::from_slice(&fs::read(path).map_err(|error| {
        Failure::config("apphook.gac.config", format!("{}: {error}", path.display()))
    })?)?;
    let binary = environment
        .get("SKARBIEC_BIN")
        .map(String::as_str)
        .unwrap_or("skarbiec");
    resolve_skarbiec(&mut value, binary)?;
    Ok(value)
}

fn python_string(value: &Path) -> String {
    serde_json::to_string(&value.to_string_lossy()).expect("path string")
}

fn render_script(model: &Path, output: &Path, rotation: &str) -> String {
    [
        "import bpy".to_string(),
        "bpy.ops.wm.read_factory_settings(use_empty=True)".into(),
        format!("bpy.ops.import_scene.gltf(filepath={})", python_string(model)),
        "scene = bpy.context.scene".into(),
        format!("for obj in scene.objects: obj.rotation_euler = {rotation}"),
        "scene.render.engine = \"BLENDER_EEVEE_NEXT\" if hasattr(bpy.types, \"BLENDER_EEVEE_NEXT\") else \"BLENDER_EEVEE\"".into(),
        "scene.render.resolution_x = 512".into(),
        "scene.render.resolution_y = 512".into(),
        format!("scene.render.filepath = {}", python_string(output)),
        "bpy.ops.render.render(write_still=True)".into(),
        "import os".into(),
        format!("print(\"rendered\", os.path.getsize({}))", python_string(output)),
    ]
    .join("\n")
}

fn render_with_blender_session(
    root: &Path,
    mcp: &Value,
    scripts: &[String],
) -> Result<(), Failure> {
    let module = root.join("pipeline/blender.js");
    if !module.exists() {
        return Err(Failure::new(
            "apphook.gac.blender",
            Code::Prerequisite,
            format!(
                "game_asset_creator Blender dependency not found: {}",
                module.display()
            ),
        ));
    }
    const ADAPTER: &str = r#"import { pathToFileURL } from 'node:url';
const decode = value => Buffer.from(value, 'base64').toString('utf8');
const [modulePath, config, ...codes] = process.argv.slice(1);
const { BlenderSession } = await import(pathToFileURL(modulePath));
const session = await BlenderSession.start(JSON.parse(decode(config)));
try { for (const code of codes) await session.execute(decode(code)); }
finally { await session.close().catch(() => {}); }"#;
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut command = Command::new("node");
    command.args(["--input-type=module", "--eval", ADAPTER]);
    command.arg(&module);
    command.arg(encode(serde_json::to_string(mcp)?.as_bytes()));
    for script in scripts {
        command.arg(encode(script.as_bytes()));
    }
    let output = command.output().map_err(|error| {
        Failure::new(
            "apphook.gac.blender",
            Code::Prerequisite,
            format!("failed to run node: {error}"),
        )
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Failure::new(
            "apphook.gac.blender",
            Code::Refused,
            if detail.is_empty() {
                format!("Blender session exited with {}", output.status)
            } else {
                detail
            },
        ));
    }
    Ok(())
}

fn score_with_brama(
    url: &str,
    key: &str,
    model: Option<&str>,
    rubric: &str,
    images: &[PathBuf],
) -> Result<Value, Failure> {
    let mut content = vec![json!({ "type": "text", "text": rubric })];
    for image in images {
        let png = fs::read(image)?;
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)) }
        }));
    }
    let endpoint = format!("{}/v1/chat/completions", url.trim_end_matches('/'));
    let body = request_json(
        "POST",
        &endpoint,
        &[
            ("content-type", "application/json".into()),
            ("authorization", format!("Bearer {key}")),
        ],
        Some(json!({
            "model": model.unwrap_or("any"),
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": content }]
        })),
    )
    .map_err(|failure| {
        if failure.detail.starts_with("HTTP ") {
            Failure::new(
                "apphook.gac.brama",
                Code::Refused,
                format!("brama {}", failure.detail),
            )
        } else {
            failure
        }
    })?;
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(start) = text.find('{') else {
        return Err(Failure::new(
            "apphook.gac.brama",
            Code::Refused,
            format!(
                "brama reply had no JSON: {}",
                text.chars().take(200).collect::<String>()
            ),
        ));
    };
    let Some(end) = text.rfind('}') else {
        return Err(Failure::new(
            "apphook.gac.brama",
            Code::Refused,
            format!(
                "brama reply had no JSON: {}",
                text.chars().take(200).collect::<String>()
            ),
        ));
    };
    serde_json::from_str(&text[start..=end]).map_err(Failure::from)
}
fn gac_root(harness: &Path, environment: &BTreeMap<String, String>) -> Result<PathBuf, Failure> {
    let configured = environment
        .get("GAC_ROOT")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            crate::manifest::load(harness, "game-asset-creator").ok().and_then(|manifest| {
                manifest.document.get("repositories")
                    .and_then(serde_yaml::Value::as_sequence)
                    .and_then(|repositories| repositories.first())
                    .and_then(|repository| repository.get("root"))
                    .and_then(serde_yaml::Value::as_str)
                    .map(PathBuf::from)
            })
        })
        .ok_or_else(|| Failure::config(
            "apphook.gac.visual-eval",
            "GAC_ROOT is required: set it to the game_asset_creator repository or declare that repository first in apps/game-asset-creator/probierz.yaml",
        ))?;
    if !configured.is_dir() {
        return Err(Failure::new(
            "apphook.gac.visual-eval",
            Code::Prerequisite,
            format!(
                "game_asset_creator dependency not found at {}; set GAC_ROOT to the product repository",
                configured.display()
            ),
        ));
    }
    Ok(configured)
}

fn gac_visual_eval(
    harness: &Path,
    args: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    let options = visual_options(args)?;
    let root = gac_root(harness, environment)?;
    let models = options
        .models
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("assets/models"));
    let output = options
        .out
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .get("PROBIERZ_ARTIFACTS")
                .filter(|value| !value.trim().is_empty())
                .map(|root| Path::new(root).join("visual-eval"))
        })
        .unwrap_or_else(|| harness.join("test-results/visual-eval"));
    let config_path = options
        .config
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("pipeline.config.json"));
    let rubric_name = options
        .rubric
        .or_else(|| environment.get("PROBIERZ_EVAL_RUBRIC").cloned())
        .unwrap_or_else(|| "rts-character".into());
    let threshold = options
        .threshold
        .as_deref()
        .unwrap_or("0.7")
        .parse::<f64>()
        .map_err(|_| Failure::invalid("apphook.gac.visual-eval", "--threshold needs a number"))?;
    let config = load_gac_config(&config_path, environment)?;
    let brama = config.pointer("/models/brama").and_then(Value::as_object);
    let url = brama
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str);
    let key = brama
        .and_then(|value| value.get("key"))
        .and_then(Value::as_str);
    if url.is_none() || key.is_none() {
        return Err(Failure::config(
            "apphook.gac.visual-eval",
            "models.brama.{url,key} missing from pipeline config (skarbiec:// refs)",
        ));
    }
    let mut glbs: Vec<PathBuf> = fs::read_dir(&models)
        .map_err(|error| {
            Failure::config(
                "apphook.gac.visual-eval",
                format!("{}: {error}", models.display()),
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("glb"))
        .collect();
    glbs.sort();
    if glbs.is_empty() {
        return Err(Failure::config(
            "apphook.gac.visual-eval",
            format!("no .glb files in {}", models.display()),
        ));
    }
    fs::create_dir_all(&output)?;
    let rubric = if rubric_name == "rts-character" {
        "You are an art director reviewing ONE low-poly RTS character render set.\nScore each dimension 0..1 and give an overall score 0..1:\n- proportions: chunky heroic low-poly (Thronefall style), not noodle-limbed\n- silhouette: readable at RTS camera distance, clear head/torso/weapon shapes\n- palette: flat-shaded colors consistent with a fantasy race (no texture noise)\n- artifacts: no z-fighting, no missing limbs, no collapsed geometry\nReply with a single JSON object: {\"proportions\": x, \"silhouette\": x, \"palette\": x, \"artifacts\": x, \"overall\": x, \"issues\": [\"...\"]}"
    } else {
        rubric_name.as_str()
    };
    let angles = [
        ("front", "(0, 0, 0)"),
        ("side", "(0, 0, 1.5708)"),
        ("back34", "(0, 0, 3.927)"),
    ];
    let empty_mcp = json!({});
    let mcp = config.pointer("/blender/mcp").unwrap_or(&empty_mcp);
    let mut results = Vec::new();
    for model_path in glbs {
        let stem = model_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("model");
        let renders: Vec<PathBuf> = angles
            .iter()
            .map(|(name, _)| output.join(format!("{stem}-{name}.png")))
            .collect();
        let scripts: Vec<String> = angles
            .iter()
            .zip(&renders)
            .map(|((_, rotation), render)| render_script(&model_path, render, rotation))
            .collect();
        render_with_blender_session(&root, mcp, &scripts)?;
        let scores = score_with_brama(
            url.expect("checked"),
            key.expect("checked"),
            brama
                .and_then(|value| value.get("model"))
                .and_then(Value::as_str),
            rubric,
            &renders,
        )?;
        let passed = scores
            .get("overall")
            .and_then(Value::as_f64)
            .is_some_and(|value| value >= threshold);
        results.push(json!({
            "asset": model_path.file_name().and_then(|value| value.to_str()).unwrap_or(""),
            "renders": renders,
            "scores": scores,
            "pass": passed
        }));
    }
    let passed = results
        .iter()
        .filter(|result| result.get("pass").and_then(Value::as_bool) == Some(true))
        .count();
    let report = json!({
        "rubric": rubric_name,
        "threshold": threshold,
        "total": results.len(),
        "passed": passed,
        "failed": results.len() - passed,
        "results": results
    });
    let report_path = output.join("eval-report.json");
    write_private(&report_path, &serde_json::to_vec_pretty(&report)?)?;
    if passed != results.len() {
        return Err(Failure::new(
            "apphook.gac.visual-eval",
            Code::Refused,
            format!(
                "visual evaluation failed: {} of {} assets failed; report: {}",
                results.len() - passed,
                results.len(),
                report_path.display()
            ),
        ));
    }
    Ok(
        json!({ "reportPath": report_path, "total": results.len(), "passed": passed, "failed": results.len() - passed }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_ids_and_fixture_shape_match_the_javascript_contract() {
        assert_eq!(
            deterministic_uuid(&["oko-e2e-org", "run-123"]),
            "dc61a7ae-b9d2-4bd0-8864-0783fa068918"
        );
        let strategy = strategy_document("run-123");
        assert_eq!(
            strategy["northStar"]["metrics"].as_array().map(Vec::len),
            Some(4)
        );
        assert_eq!(strategy["initiatives"].as_array().map(Vec::len), Some(13));
        let capacity: f64 = strategy["initiatives"]
            .as_array()
            .expect("initiatives")
            .iter()
            .filter_map(|item| item.get("capacityPercent").and_then(Value::as_f64))
            .sum();
        assert!((capacity - 100.0).abs() < 0.0001);
    }

    #[test]
    fn fixture_builder_writes_real_glb_headers_and_private_config() {
        let harness =
            std::env::temp_dir().join(format!("probierz-gac-hook-{}", std::process::id()));
        let result = gac_fixtures(&harness, &BTreeMap::new()).expect("fixtures");
        let directory = harness.join("test-results/game-asset-creator/fixtures");
        assert_eq!(result["dir"].as_str(), directory.to_str());
        let valid = fs::read(result["valid"].as_str().expect("valid path")).expect("valid GLB");
        assert_eq!(&valid[..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(valid[4..8].try_into().expect("version")),
            2
        );
        assert_eq!(
            u32::from_le_bytes(valid[8..12].try_into().expect("length")) as usize,
            valid.len()
        );
        assert_eq!(
            fs::read(result["corrupt"].as_str().expect("corrupt path")).expect("corrupt"),
            b"definitely not a glb file"
        );
        fs::remove_dir_all(harness).expect("cleanup");
    }

    #[test]
    fn autonomy_only_seed_refuses_no_external_dependency() {
        let environment =
            BTreeMap::from([("PROBIERZ_JOURNEYS".into(), "autonomy-experimental".into())]);
        assert_eq!(
            oko_seed(&environment).expect("skip"),
            json!({ "skipped": "autonomy journey uses isolated local fixtures" })
        );
    }
}
