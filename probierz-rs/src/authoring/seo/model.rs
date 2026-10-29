use crate::authoring::*;
use serde_json::json;
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
    // An adjudication judges the two evaluations it was given; an evaluation
    // judges the brief, routes and page evidence.
    let compact = match adjudication {
        Some(adjudication) => adjudication.clone(),
        None => json!({
            "approvedBrief": brief, "routes": policy["routes"], "pageEvidence": evidence, "deterministic": deterministic
        }),
    }
    .to_string();
    let role = match adjudication {
        Some(_) => "You are the adjudicator for two independent Probierz SEO content evaluations.",
        None => "You are an independent Probierz SEO content evaluator.",
    };
    // The evidence bound is the policy's stated `maxEvidenceCharacters`;
    // a policy that states none sends the evidence whole.
    if let Some(maximum) = policy
        .pointer("/model/maxEvidenceCharacters")
        .and_then(JsonValue::as_u64)
    {
        if compact.len() as u64 > maximum {
            return Err(Failure::config(
                "seo-evaluate.model",
                format!(
                    "SEO model evidence is {} characters, over the {maximum} character policy limit",
                    compact.len()
                ),
            ));
        }
    }
    let instructions: Vec<&str> = policy["modelInstructions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect();
    let mut request = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": format!("{role}\n{}\nCall record_seo_content_evaluation exactly once and return no prose outside the tool call.",
                instructions.join("\n")) },
            { "role": "user", "content": [{ "type": "text", "text": compact }] }
        ],
        "tools": [seo_model_tool(policy)]
    });
    // The answer's length is the policy's stated budget, else the routed
    // model's own limit.
    if let Some(budget) = policy
        .pointer("/model/maxOutputTokens")
        .and_then(JsonValue::as_u64)
    {
        request["max_tokens"] = json!(budget);
    }
    let body = request.to_string();
    let request_sha = crate::evidence::sha256_bytes(body.as_bytes());
    let (status, raw) = post_router(url, token, agent_id, agent_secret, &body)
        .map_err(|detail| Failure::unavailable("seo-evaluate.model", detail))?;
    let payload: JsonValue = serde_json::from_str(&raw).map_err(|error| {
        Failure::unavailable(
            "seo-evaluate.model",
            format!(
                "SEO model router returned non-JSON ({status}, {error}): {}",
                raw.trim()
            ),
        )
    })?;
    let answered = http::StatusCode::from_u16(status)
        .is_ok_and(|code| code.is_success() || code.is_redirection());
    if !answered {
        return Err(Failure::unavailable(
            "seo-evaluate.model",
            router_refusal(status, &payload, &raw),
        ));
    }
    let evaluation = tool_call_arguments(&payload, "record_seo_content_evaluation")
        .map_err(|detail| Failure::config("seo-evaluate.model", detail))?;
    if evaluation
        .get("summary")
        .and_then(JsonValue::as_str)
        .is_none_or(|summary| summary.trim().is_empty())
    {
        return Err(Failure::config(
            "seo-evaluate.model",
            "SEO model evaluation summary is required",
        ));
    }
    Ok(json!({
        "modelRequested": model,
        "responseSha256": crate::evidence::sha256_bytes(raw.as_bytes()),
        "modelReturned": payload.get("model"),
        "requestSha256": request_sha,
        "rubricSha256": crate::evidence::sha256_bytes(json!({ "dimensions": policy["dimensions"], "instructions": policy["modelInstructions"] }).to_string().as_bytes()),
        "usage": payload.get("usage"),
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
                        JsonValue::String(key.to_string()),
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
    // A PEM key announces itself with its BEGIN line; anything else is DER.
    let invalid = |error: &dyn std::fmt::Display| {
        Failure::config(
            "seo-evaluate.sign",
            format!("invalid Ed25519 private key: {error}"),
        )
    };
    let signing = if text.contains("BEGIN") {
        SigningKey::from_pkcs8_pem(text.trim()).map_err(|error| invalid(&error))?
    } else {
        SigningKey::from_pkcs8_der(key).map_err(|error| invalid(&error))?
    };
    let public = signing.verifying_key();
    let canonical = canonical_json(payload);
    let signature = signing.sign(canonical.as_bytes());
    let der = public
        .to_public_key_der()
        .map_err(|error| Failure::config("seo-evaluate.sign", error.to_string()))?;
    Ok(json!({
        "algorithm": "Ed25519",
        "payloadSha256": crate::evidence::sha256_bytes(canonical.as_bytes()),
        "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
        "publicKeyPem": public.to_public_key_pem(LineEnding::LF).map_err(|error| Failure::config("seo-evaluate.sign", error.to_string()))?,
        "publicKeyFingerprintSha256": crate::evidence::sha256_bytes(der.as_bytes())
    }))
}
