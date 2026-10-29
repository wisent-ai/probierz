use crate::authoring::*;
use hmac::{Hmac, Mac};
pub(crate) fn selected_setting(
    loaded: Option<&manifest::Manifest>,
    target: Option<&str>,
    name: &str,
    explicit: Option<&str>,
) -> Option<String> {
    if let Some(value) = explicit {
        return Some(value.trim().to_string());
    }
    if let Some(value) = loaded.and_then(|manifest| {
        let target = target?;
        manifest
            .document
            .get("surfaces")?
            .get(target)?
            .get("conditions")?
            .get(name)?
            .as_str()
    }) {
        return Some(value.trim().to_string());
    }
    // A setting the environment holds in bytes that are not UTF-8 is kept,
    // shown lossily, so the check it reaches refuses it by its value rather
    // than the setting being read as never given.
    std::env::var_os(name).map(|value| value.to_string_lossy().trim().to_string())
}

/// The router URL authoring calls: the one check `stado submit` uses, which
/// judges the host from the parsed URL, so 127.attacker.example is not taken
/// for this machine and sent the router token over plain HTTP.
pub(crate) fn stado_model_router_url(raw: Option<&str>) -> Result<String, String> {
    crate::stado::model_router_url(raw).map_err(|failure| failure.detail)
}

pub(crate) fn required_setting(value: Option<String>, name: &str) -> Result<String, String> {
    value
        .filter(|item| !item.trim().is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

/// The request signature the model router checks: HMAC-SHA256 of `message`
/// keyed by `secret`, hex-encoded.
pub(crate) fn hmac_sha256(secret: &[u8], message: &[u8]) -> Result<String, String> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret)
        .map_err(|error| format!("the agent secret cannot key HMAC-SHA256: {error}"))?;
    mac.update(message);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

pub(crate) struct RouterReply {
    pub(crate) content: String,
    pub(crate) model: JsonValue,
    pub(crate) usage: JsonValue,
}

/// One chat completion through the Stado model router. The call waits for the
/// router's own answer: the router owns provider deadlines and names its
/// failure, so a clock here would only end a slow honest answer early. The
/// bearer and the agent signature travel only in the request's headers; no
/// file holding them is ever written.
pub(crate) fn post_router(
    url: &str,
    token: &str,
    agent_id: &str,
    agent_secret: &str,
    body: &str,
) -> Result<(u16, String), String> {
    if token.trim().is_empty() {
        return Err("STADO_MODEL_ROUTER_TOKEN is required".to_string());
    }
    if token.chars().any(char::is_whitespace) {
        return Err("STADO_MODEL_ROUTER_TOKEN must not contain whitespace".to_string());
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(None)
        .build()
        .map_err(|error| format!("model router client cannot be built: {error}"))?;
    let endpoint = format!("{url}/v1/chat/completions");
    let mut request = client
        .post(&endpoint)
        .bearer_auth(token)
        .header("content-type", "application/json");
    if !agent_id.trim().is_empty() || !agent_secret.trim().is_empty() {
        if agent_id.trim().is_empty() || agent_secret.trim().is_empty() {
            return Err("agent identity needs both an agent ID and an agent secret".to_string());
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs()
            .to_string();
        let digest = crate::evidence::sha256_bytes(body.as_bytes());
        let signature = hmac_sha256(
            agent_secret.as_bytes(),
            format!("{agent_id}:{timestamp}:{digest}").as_bytes(),
        )?;
        request = request
            .header("x-agent-id", agent_id)
            .header("x-agent-timestamp", timestamp)
            .header("x-agent-signature", signature);
    }
    let response = stado_wait::blocking(stado_wait::Kind::Network, "model router request", &endpoint, || {
        request.body(body.to_string()).send()
    })
    .map_err(|error| format!("model router request to {endpoint} failed: {error}"))?;
    let status = response.status().as_u16();
    let payload = response
        .text()
        .map_err(|error| format!("model router answer HTTP {status} cannot be read: {error}"))?;
    Ok((status, payload))
}

/// The model router's refusal: its `error.message` when it names one, else
/// the whole body it answered, so the cause is never replaced by a guess.
pub(crate) fn router_refusal(status: u16, payload: &JsonValue, raw: &str) -> String {
    match payload
        .pointer("/error/message")
        .and_then(JsonValue::as_str)
    {
        Some(message) => format!("model router HTTP {status}: {message}"),
        None => format!("model router HTTP {status}: {}", raw.trim()),
    }
}

/// The arguments of the one `tool_name` call in the router's answer. An
/// answer with no such call, several, or arguments that are not a JSON
/// object is refused naming what it held; prose is never searched for an
/// object in its place.
pub(crate) fn tool_call_arguments(
    payload: &JsonValue,
    tool_name: &str,
) -> Result<JsonValue, String> {
    let calls: Vec<&JsonValue> = payload
        .pointer("/choices/0/message/tool_calls")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter(|call| {
            call.get("type").and_then(JsonValue::as_str) == Some("function")
                && call.pointer("/function/name").and_then(JsonValue::as_str) == Some(tool_name)
        })
        .collect();
    let [call] = calls.as_slice() else {
        return Err(format!(
            "the model router answered {} {tool_name} tool calls where exactly one is required: {payload}",
            calls.len()
        ));
    };
    let arguments = call
        .pointer("/function/arguments")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!("the model router's {tool_name} call carries no arguments: {call}")
        })?;
    match serde_json::from_str::<JsonValue>(arguments) {
        Ok(object @ JsonValue::Object(_)) => Ok(object),
        Ok(other) => Err(format!(
            "the model router's {tool_name} arguments are not an object: {other}"
        )),
        Err(error) => Err(format!(
            "the model router's {tool_name} arguments are not JSON ({error}): {arguments}"
        )),
    }
}
