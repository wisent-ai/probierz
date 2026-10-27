use crate::authoring::*;
use hmac::{Hmac, Mac};
use std::os::unix::fs::OpenOptionsExt;
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

/// Remove a temporary this module wrote; the sentence saying it stays on
/// disk when it cannot be removed.
fn discard(path: &std::path::Path) -> Option<String> {
    fs::remove_file(path)
        .err()
        .map(|error| format!("{} is left behind: {error}", path.display()))
}

/// `cause`, with the half-written `file` taken back, or said to stay.
fn abandoned(file: &std::path::Path, cause: impl std::fmt::Display) -> String {
    match discard(file) {
        Some(left) => format!("{cause}; {left}"),
        None => cause.to_string(),
    }
}

/// A file holding `content` that only this user can read: it is created
/// owner-only, never readable by others between creation and a later chmod,
/// because the router headers it holds carry the bearer token.
pub(crate) fn temp_file(label: &str, content: &[u8]) -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let file =
        std::env::temp_dir().join(format!("probierz-{label}-{}-{stamp}", std::process::id()));
    let mut handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(u32::from(libc::S_IRUSR | libc::S_IWUSR))
        .open(&file)
        .map_err(|error| format!("{} cannot be created: {error}", file.display()))?;
    handle
        .write_all(content)
        .map_err(|error| abandoned(&file, error))?;
    Ok(file)
}

/// One chat completion through the Stado model router. The call waits for the
/// router's own answer: the router owns provider deadlines and names its
/// failure, so a clock here would only end a slow honest answer early.
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
    let mut headers = format!("Authorization: Bearer {token}\nContent-Type: application/json\n");
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
        headers.push_str(&format!("x-agent-id: {agent_id}\nx-agent-timestamp: {timestamp}\nx-agent-signature: {signature}\n"));
    }
    let header_file = temp_file("router-headers", headers.as_bytes())?;
    let body_file = temp_file("router-body", body.as_bytes())
        .map_err(|error| abandoned(&header_file, error))?;
    let output = stado_wait::output(
        Command::new("curl")
            .args(["--silent", "--show-error", "--header"])
            .arg(format!("@{}", header_file.display()))
            .args(["--data-binary"])
            .arg(format!("@{}", body_file.display()))
            .args([
                "--write-out",
                "\n%{http_code}",
                &format!("{url}/v1/chat/completions"),
            ]),
    );
    // The header file carries the router's bearer token: one left on disk is
    // refused by name, never dropped behind a successful answer.
    let left: Vec<String> = [header_file.as_path(), body_file.as_path()]
        .into_iter()
        .filter_map(discard)
        .collect();
    if !left.is_empty() {
        return Err(format!(
            "model router request temporaries: {}",
            left.join("; ")
        ));
    }
    let output = output.map_err(|error| format!("model router request failed: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (payload, status) = text
        .rsplit_once('\n')
        .ok_or_else(|| "model router returned no HTTP status".to_string())?;
    let status = status
        .parse::<u16>()
        .map_err(|error| format!("model router returned an invalid HTTP status {status:?}: {error}"))?;
    Ok((status, payload.to_string()))
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
