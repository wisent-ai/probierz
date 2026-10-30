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

/// Where authoring's model work goes. A router URL the caller, the app's
/// surface conditions or `STADO_MODEL_ROUTER_URL` names is used as named;
/// with none named Probierz never dials Brama itself: Stado's capability
/// resolution (`stado capabilities resolve inference --consumer probierz`)
/// hands out Brama's route, or the OpenAI-compatible endpoint declared in its
/// place with its model and the role of the item holding its key.
pub(crate) struct ModelRoute {
    pub(crate) url: String,
    /// The endpoint answering in Brama's place, when Stado handed one out.
    pub(crate) in_place: Option<InPlace>,
}

/// The endpoint Stado hands out in Brama's place: its provider, the model it
/// declares and the key its role holds. It reads no agent signature.
pub(crate) struct InPlace {
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) key: String,
}

/// The consumer Probierz asks Stado's capability resolution as.
const CONSUMER: &str = "probierz";

impl ModelRoute {
    /// The route for `raw`, the URL named for this work, checked as
    /// `stado submit` checks it (the host judged from the parsed URL, so
    /// 127.attacker.example is not taken for this machine and sent the
    /// router token over plain HTTP); Stado's resolution when none is named.
    pub(crate) fn select(raw: Option<&str>) -> Result<Self, String> {
        if raw.is_some_and(|value| !value.trim().is_empty()) {
            let url = crate::stado::model_router_url(raw).map_err(|failure| failure.detail)?;
            return Ok(Self {
                url,
                in_place: None,
            });
        }
        resolve()
    }

    /// The bearer this route is sent: the endpoint's own key in Brama's
    /// place, else the router token the caller names.
    pub(crate) fn bearer(&self, named: Option<String>) -> Option<String> {
        match &self.in_place {
            Some(endpoint) => Some(endpoint.key.clone()),
            None => named,
        }
    }

    /// The model asked for: the one the caller names, else the model the
    /// endpoint in Brama's place declares.
    pub(crate) fn model(&self, named: Option<String>) -> Option<String> {
        named.filter(|model| !model.trim().is_empty()).or_else(|| {
            self.in_place
                .as_ref()
                .map(|endpoint| endpoint.model.clone())
        })
    }

    /// Whether requests carry Probierz's agent signature: Brama reads it;
    /// an endpoint in its place is never sent one.
    pub(crate) fn signs(&self) -> bool {
        self.in_place.is_none()
    }
}

/// One Stado command's output, or the refusal naming it.
fn stado(words: &[&str]) -> Result<std::process::Output, String> {
    stado_wait::output(Command::new(crate::stado::STADO_BIN).args(words)).map_err(|error| {
        format!(
            "STADO_MODEL_ROUTER_URL is not set and `stado {}` could not run: {error}; name the model router or install Stado",
            words.join(" ")
        )
    })
}

/// Ask Stado where authoring's model work goes.
fn resolve() -> Result<ModelRoute, String> {
    let words = [
        "capabilities",
        "resolve",
        "inference",
        "--consumer",
        CONSUMER,
        "--json",
    ];
    let output = stado(&words)?;
    if !output.status.success() {
        let named = [&output.stdout, &output.stderr]
            .into_iter()
            .filter_map(|bytes| serde_json::from_slice::<JsonValue>(bytes).ok())
            .find_map(|envelope| {
                envelope
                    .pointer("/details/unreachable")?
                    .as_array()
                    .cloned()
            })
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        format!(
                            "{} at {}: {}",
                            item["product"].as_str().unwrap_or("an alternative"),
                            item["route"].as_str().unwrap_or("its route"),
                            item["cause"].as_str().unwrap_or("no cause given")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .filter(|named| !named.is_empty());
        return Err(match named {
            Some(named) => format!(
                "STADO_MODEL_ROUTER_URL is not set and no inference alternative answers {CONSUMER}: {named}; name the model router, or declare an endpoint in Brama's place (stado config set capabilities.inference.endpoint.url <url>, capabilities.inference.endpoint.model <model> and capabilities.inference.endpoint.key_role <role>)"
            ),
            None => format!(
                "STADO_MODEL_ROUTER_URL is not set and `stado {}` exited {}: {}",
                words.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    let answer: JsonValue = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "`stado {}` printed an answer that is not JSON: {error}",
            words.join(" ")
        )
    })?;
    let text = |field: &str| {
        answer[field]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let (Some(provider), Some(route)) = (text("provider"), text("route")) else {
        return Err(format!(
            "`stado {}` answered no provider and route: {answer}",
            words.join(" ")
        ));
    };
    let url = crate::stado::model_router_url(Some(&route)).map_err(|failure| {
        format!(
            "the {provider} route Stado resolved is refused: {}",
            failure.detail
        )
    })?;
    if provider == "brama" {
        return Ok(ModelRoute {
            url,
            in_place: None,
        });
    }
    let model = text("model").ok_or_else(|| {
        format!(
            "{provider} at {route} answers in Brama's place but declares no model: set capabilities.inference.endpoint.model"
        )
    })?;
    // Authoring's model boundary is authenticated: an endpoint is sent the
    // key its role holds, never nothing and never the router's token.
    let role = text("key_role").ok_or_else(|| {
        format!(
            "{provider} at {route} answers in Brama's place but names no key role; Probierz sends every model request a bearer: set capabilities.inference.endpoint.key_role"
        )
    })?;
    let words = ["credentials", "get", "--role", &role, "--field", "token"];
    let output = stado(&words)?;
    if !output.status.success() {
        return Err(format!(
            "{provider} at {route} answers in Brama's place, but `stado {}` exited {}: {}",
            words.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let key = String::from_utf8(output.stdout)
        .map_err(|_| format!("the key of role {role} is not UTF-8"))?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    if key.is_empty() {
        return Err(format!(
            "the item playing role {role} holds no value in field token"
        ));
    }
    Ok(ModelRoute {
        url,
        in_place: Some(InPlace {
            provider,
            model,
            key,
        }),
    })
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
    let response = stado_wait::blocking(
        stado_wait::Kind::Network,
        "model router request",
        &endpoint,
        || request.body(body.to_string()).send(),
    )
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
