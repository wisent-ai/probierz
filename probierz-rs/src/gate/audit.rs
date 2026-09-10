use crate::gate::*;
pub(crate) fn random_uuid() -> Result<String, Failure> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let encoded = hex::encode(bytes);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &encoded[0..8],
        &encoded[8..12],
        &encoded[12..16],
        &encoded[16..20],
        &encoded[20..32]
    ))
}

pub(crate) fn redact(value: Value, key: &str) -> Value {
    let lower = key.to_ascii_lowercase();
    if [
        "auth",
        "cookie",
        "credential",
        "email",
        "key",
        "otp",
        "password",
        "pii",
        "secret",
        "session",
        "token",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Value::String("[REDACTED]".to_string());
    }
    match value {
        Value::Array(values) => {
            Value::Array(values.into_iter().map(|value| redact(value, "")).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(name, value)| {
                    let redacted = redact(value, &name);
                    (name, redacted)
                })
                .collect(),
        ),
        other => other,
    }
}

pub(crate) fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

pub(crate) fn audit_access(
    harness: &Path,
    action: &str,
    outcome: &str,
    app_id: Option<&str>,
    resource: Option<&str>,
    details: Value,
) -> Result<(), Failure> {
    let at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let event_id = random_uuid()?;
    let actor = std::env::var("PROBIERZ_ACTOR")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("GITHUB_ACTOR")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| std::env::var("USER").ok().filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "unknown".to_string());
    let payload = object([
        ("schemaVersion", Value::from(1)),
        ("kind", Value::String("probierz-access-audit".to_string())),
        ("eventId", Value::String(event_id.clone())),
        ("at", Value::String(at.clone())),
        ("actor", Value::String(actor)),
        ("action", Value::String(action.to_string())),
        ("outcome", Value::String(outcome.to_string())),
        (
            "appId",
            app_id
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
        ),
        ("runId", Value::Null),
        (
            "resource",
            resource
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
        ),
        (
            "context",
            object([
                ("ci", Value::Bool(nonempty_env("CI").is_some())),
                (
                    "workflow",
                    nonempty_env("GITHUB_WORKFLOW")
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                ),
                (
                    "job",
                    nonempty_env("GITHUB_JOB")
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                ),
            ]),
        ),
        ("details", redact(details, "")),
    ]);
    let hash = hex::encode(Sha256::digest(canonical(&payload).as_bytes()));
    let mut record = payload;
    record
        .as_object_mut()
        .ok_or_else(|| Failure::config("gate.audit", "audit payload is not an object"))?
        .insert("sha256".to_string(), Value::String(hash));
    let directory = harness.join("test-results").join(".audit").join(&at[..10]);
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    let timestamp = at.replace([':', '.'], "-");
    let file = directory.join(format!("{timestamp}-{event_id}.json"));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(file)?;
    output.write_all(serde_json::to_string_pretty(&record)?.as_bytes())?;
    output.write_all(b"\n")?;
    Ok(())
}

