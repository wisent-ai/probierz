use serde_json::json;
use crate::apphooks::*;
pub(crate) fn ensure_technical_account(source: &BTreeMap<String, String>) -> Result<Value, Failure> {
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

pub(crate) fn generate_admin_otp(source: &BTreeMap<String, String>) -> Result<String, Failure> {
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

pub(crate) fn validated_otp(value: Option<&Value>, source_name: &str) -> Result<String, Failure> {
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

pub(crate) fn oko_wait_for_otp(
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

pub(crate) fn otp_options(args: &[String]) -> Result<(DateTime<Utc>, Duration), Failure> {
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

pub(crate) fn state_path(source: &BTreeMap<String, String>) -> PathBuf {
    let root = source
        .get("PROBIERZ_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("test-results/oko-local"));
    root.join("diagnostics/oko-seed-state.json")
}

pub(crate) fn write_state(path: &Path, state: &Value) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(state)?;
    write_private(path, &bytes)?;
    Ok(())
}

pub(crate) fn read_state(source: &BTreeMap<String, String>) -> Result<(PathBuf, Value), Failure> {
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

pub(crate) fn delete_organization(source: &BTreeMap<String, String>, org_id: &str) -> Result<(), Failure> {
    supabase(
        source,
        &format!("/rest/v1/organizations?id=eq.{org_id}"),
        "DELETE",
        Some("return=minimal"),
        None,
    )?;
    Ok(())
}

