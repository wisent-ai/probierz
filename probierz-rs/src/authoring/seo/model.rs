use serde_json::json;
use crate::authoring::*;
pub(crate) fn invoke_seo_model(
    model: &str,
    url: &str,
    token: &str,
    agent_id: &str,
    agent_secret: &str,
    policy: &JsonValue,
    brief: &JsonValue,
    evidence: &JsonValue,
    deterministic: &JsonValue,
    adjudication: Option<&JsonValue>,
) -> Result<JsonValue, Failure> {
    let compact = adjudication.cloned().unwrap_or_else(|| json!({
        "approvedBrief": brief, "routes": policy["routes"], "pageEvidence": evidence, "deterministic": deterministic
    })).to_string();
    let maximum = policy
        .pointer("/model/maxEvidenceCharacters")
        .and_then(JsonValue::as_u64)
        .unwrap_or(500_000) as usize;
    if compact.len() > maximum {
        return Err(Failure::config(
            "seo-evaluate.model",
            format!(
                "SEO model evidence is {} characters, over the {maximum} character policy limit",
                compact.len()
            ),
        ));
    }
    let instructions: Vec<&str> = policy["modelInstructions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect();
    let body = json!({
        "model": model,
        "max_tokens": policy.pointer("/model/maxOutputTokens").and_then(JsonValue::as_u64).unwrap_or(3200),
        "temperature": 0,
        "messages": [
            { "role": "system", "content": format!("{}\n{}\nCall record_seo_content_evaluation exactly once and return no prose outside the tool call.",
                if adjudication.is_some() { "You are the adjudicator for two independent Probierz SEO content evaluations." } else { "You are an independent Probierz SEO content evaluator." },
                instructions.join("\n")) },
            { "role": "user", "content": [{ "type": "text", "text": compact }] }
        ],
        "tools": [seo_model_tool(policy)]
    }).to_string();
    let request_sha = hex::encode(Sha256::digest(body.as_bytes()));
    let (status, raw) = post_router(url, token, agent_id, agent_secret, &body, 120)
        .map_err(|detail| Failure::unavailable("seo-evaluate.model", detail))?;
    let payload: JsonValue = serde_json::from_str(&raw).map_err(|_| {
        Failure::unavailable(
            "seo-evaluate.model",
            format!("SEO model router returned non-JSON ({status})"),
        )
    })?;
    if !(200..400).contains(&status) {
        return Err(Failure::unavailable(
            "seo-evaluate.model",
            format!(
                "SEO model router HTTP {status}: {}",
                payload
                    .pointer("/error/message")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("request failed")
                    .chars()
                    .take(500)
                    .collect::<String>()
            ),
        ));
    }
    let calls: Vec<&JsonValue> = payload
        .pointer("/choices/0/message/tool_calls")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter(|call| {
            call.get("type").and_then(JsonValue::as_str) == Some("function")
                && call.pointer("/function/name").and_then(JsonValue::as_str)
                    == Some("record_seo_content_evaluation")
        })
        .collect();
    if calls.len() != 1 {
        return Err(Failure::config(
            "seo-evaluate.model",
            "SEO model router must return exactly one record_seo_content_evaluation tool call",
        ));
    }
    let evaluation: JsonValue = serde_json::from_str(
        calls[0]
            .pointer("/function/arguments")
            .and_then(JsonValue::as_str)
            .unwrap_or_default(),
    )
    .map_err(|_| {
        Failure::config(
            "seo-evaluate.model",
            "SEO model router returned invalid tool arguments",
        )
    })?;
    if evaluation
        .get("summary")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Err(Failure::config(
            "seo-evaluate.model",
            "SEO model evaluation summary is required",
        ));
    }
    Ok(json!({
        "modelRequested": model,
        "responseSha256": hex::encode(Sha256::digest(raw.as_bytes())),
        "modelReturned": payload.get("model").cloned().unwrap_or(JsonValue::Null),
        "requestSha256": request_sha,
        "rubricSha256": hex::encode(Sha256::digest(json!({ "dimensions": policy["dimensions"], "instructions": policy["modelInstructions"] }).to_string().as_bytes())),
        "usage": payload.get("usage").cloned().unwrap_or(JsonValue::Null),
        "evaluation": evaluation
    }))
}

pub(crate) fn canonical_json(value: &JsonValue) -> String {
    match value {
        JsonValue::Object(map) => {
            let ordered: BTreeMap<&str, &JsonValue> = map
                .iter()
                .map(|(key, value)| (key.as_str(), value))
                .collect();
            format!(
                "{{{}}}",
                ordered
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        JsonValue::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => value.to_string(),
    }
}

pub(crate) fn sign_seo_payload(payload: &JsonValue, key: &[u8]) -> Result<JsonValue, Failure> {
    let text = String::from_utf8_lossy(key);
    let signing = if text.contains("BEGIN") {
        SigningKey::from_pkcs8_pem(text.trim()).map_err(|error| {
            Failure::config(
                "seo-evaluate.sign",
                format!("invalid Ed25519 private key: {error}"),
            )
        })?
    } else {
        SigningKey::from_pkcs8_der(key).map_err(|error| {
            Failure::config(
                "seo-evaluate.sign",
                format!("invalid Ed25519 private key: {error}"),
            )
        })?
    };
    let public = signing.verifying_key();
    let canonical = canonical_json(payload);
    let signature = signing.sign(canonical.as_bytes());
    let der = public
        .to_public_key_der()
        .map_err(|error| Failure::config("seo-evaluate.sign", error.to_string()))?;
    Ok(json!({
        "algorithm": "Ed25519",
        "payloadSha256": hex::encode(Sha256::digest(canonical.as_bytes())),
        "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
        "publicKeyPem": public.to_public_key_pem(LineEnding::LF).map_err(|error| Failure::config("seo-evaluate.sign", error.to_string()))?,
        "publicKeyFingerprintSha256": hex::encode(Sha256::digest(der.as_bytes()))
    }))
}

