use serde_json::json;
use crate::evidence::*;
pub fn verify_receipt_value(
    file: &Path,
    trusted_public_key: Option<&Path>,
    expected_fingerprint: Option<&str>,
) -> Result<Value, Failure> {
    let receipt_value = json_file(file)?;
    let mut payload_map = receipt_value
        .as_object()
        .cloned()
        .ok_or_else(|| Failure::invalid("evidence.verify_receipt", "receipt is not an object"))?;
    let signing = payload_map.remove("signing").ok_or_else(|| {
        Failure::invalid(
            "evidence.verify_receipt",
            "unsupported or missing receipt signature",
        )
    })?;
    if signing.get("algorithm").and_then(Value::as_str) != Some("Ed25519") {
        return Err(Failure::invalid(
            "evidence.verify_receipt",
            "unsupported or missing receipt signature",
        ));
    }
    let public = match trusted_public_key {
        Some(path) => verifying_key(&fs::read(path)?)?,
        None => verifying_key(
            signing
                .get("publicKeyPem")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .as_bytes(),
        )?,
    };
    let fingerprint = sha256_bytes(&public_der(&public)?);
    let expected = expected_fingerprint
        .map(str::to_string)
        .or_else(|| std::env::var("PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT").ok());
    let canonical_payload = canonical(&Value::Object(payload_map.clone()));
    let signature_valid = BASE64
        .decode(
            signing
                .get("signature")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .is_some_and(|signature| {
            public
                .verify(canonical_payload.as_bytes(), &signature)
                .is_ok()
        });
    let trusted = trusted_public_key.is_some()
        || expected
            .as_deref()
            .is_some_and(|value| value == fingerprint);
    let payload_hash = sha256_bytes(canonical_payload.as_bytes());
    let valid = signature_valid
        && trusted
        && signing.get("payloadSha256").and_then(Value::as_str) == Some(&payload_hash);
    let payload = Value::Object(payload_map);
    let mut answer = Map::new();
    answer.insert("valid".into(), json!(valid));
    answer.insert("signatureValid".into(), json!(signature_valid));
    answer.insert("trusted".into(), json!(trusted));
    answer.insert("fingerprint".into(), json!(fingerprint));
    answer.insert("payloadSha256".into(), json!(payload_hash));
    answer.insert(
        "receiptId".into(),
        json!(signed_evidence_id(&payload, &signing)),
    );
    for key in [
        "issuedAt",
        "productId",
        "verdict",
        "appId",
        "release",
        "expectedHarnessSha",
        "expectedSourceSha",
        "builds",
    ] {
        if key == "productId" {
            if let Some(value) = payload.get("productId").or_else(|| payload.get("appId")) {
                answer.insert(key.into(), value.clone());
            }
        } else if let Some(value) = payload.get(key) {
            answer.insert(key.into(), value.clone());
        }
    }
    answer.insert(
        "secretScans".into(),
        payload
            .get("secretScans")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    if let Some(value) = payload.get("policy") {
        answer.insert("policy".into(), value.clone());
    }
    let runs = payload.get("runs").cloned().unwrap_or_else(|| json!([]));
    answer.insert("runs".into(), runs.clone());
    answer.insert(
        "runIds".into(),
        Value::Array(
            runs.as_array()
                .into_iter()
                .flatten()
                .map(|run| run.get("runId").cloned().unwrap_or(Value::Null))
                .collect(),
        ),
    );
    Ok(Value::Object(answer))
}

pub fn verify_receipt(
    file: Option<&Path>,
    public_key: Option<&Path>,
    fingerprint: Option<&str>,
) -> Answer {
    let file = file.ok_or_else(|| {
        Failure::invalid("evidence.verify_receipt", "verify-receipt needs a file")
    })?;
    let result = verify_receipt_value(file, public_key, fingerprint)?;
    print_json(&result)?;
    if result.get("valid").and_then(Value::as_bool) != Some(true) {
        std::process::exit(1);
    }
    Ok(())
}

pub(crate) fn require(condition: bool, message: impl Into<String>) -> Result<(), Failure> {
    if condition {
        Ok(())
    } else {
        Err(Failure::invalid(
            "evidence.publication",
            format!("publication rejected: {}", message.into()),
        ))
    }
}

pub(crate) fn require_string<'a>(value: Option<&'a Value>, message: &str) -> Result<&'a str, Failure> {
    let text = value.and_then(Value::as_str).unwrap_or_default();
    require(!text.is_empty(), message)?;
    Ok(text)
}

pub(crate) fn parse_iso(value: &str, name: &str) -> Result<DateTime<Utc>, Failure> {
    DateTime::parse_from_rfc3339(value)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|_| {
            Failure::invalid(
                "evidence.publication",
                format!("publication rejected: {name} must be an ISO timestamp"),
            )
        })
}

pub(crate) fn immutable_storage_url(value: &str, require_path: bool) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.host_str().is_some()
            && (!require_path || url.path() != "/")
    })
}

pub(crate) fn source_revision(source: Option<&Value>) -> Option<&str> {
    let repositories = source?.get("repositories")?.as_array()?;
    repositories
        .iter()
        .find(|entry| entry.get("index").and_then(Value::as_i64) == Some(0))
        .or_else(|| repositories.first())?
        .get("gitSha")?
        .as_str()
}

