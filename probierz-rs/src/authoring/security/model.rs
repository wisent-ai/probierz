//! The model router named for the run, or Stado's inference alternatives,
//! plans declared checks and explains observations; it never executes code.
use crate::authoring::{post_router, required_setting, router_refusal, ModelRoute};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub struct Model {
    route: ModelRoute,
    token: String,
    /// The agent id and secret a named router's requests are signed with;
    /// none through Stado, which signs what it sends itself.
    signer: Option<(String, String)>,
    pub name: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub checks: Vec<String>,
}

impl Model {
    pub fn new(
        name: &str,
        url: Option<&str>,
        bearer: Option<&str>,
        agent: Option<&str>,
        secret: Option<&str>,
    ) -> Result<Self, String> {
        fn setting(explicit: Option<&str>, key: &str) -> Result<Option<String>, String> {
            if let Some(value) = explicit {
                return Ok(Some(value.trim().to_owned()));
            }
            match std::env::var(key) {
                Ok(value) => Ok(Some(value.trim().to_owned())),
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(error) => Err(format!("{key}: {error}")),
            }
        }
        if name.trim().is_empty() {
            return Err("--model must name a model route".into());
        }
        let route = ModelRoute::select(setting(url, "STADO_MODEL_ROUTER_URL")?.as_deref())?;
        // Stado presents the bearer the alternative it asks needs.
        let token = if route.needs_bearer() {
            required_setting(
                setting(bearer, "STADO_MODEL_ROUTER_TOKEN")?,
                "STADO_MODEL_ROUTER_TOKEN",
            )?
        } else {
            String::new()
        };
        let signer = if route.signs() {
            Some((
                required_setting(
                    setting(agent, "PROBIERZ_MODEL_AGENT_ID")?,
                    "PROBIERZ_MODEL_AGENT_ID",
                )?,
                required_setting(
                    setting(secret, "PROBIERZ_MODEL_AGENT_SECRET")?,
                    "PROBIERZ_MODEL_AGENT_SECRET",
                )?,
            ))
        } else {
            None
        };
        Ok(Self {
            route,
            token,
            signer,
            name: name.into(),
        })
    }
    pub fn ask(&self, instruction: &str, data: &Value) -> Result<Value, String> {
        let body = json!({"model": self.name, "messages": [
            {"role": "system", "content": instruction}, {"role": "user", "content": data.to_string()}
        ]}).to_string();
        let digest = crate::evidence::sha256_bytes(body.as_bytes());
        let (agent, secret) = self
            .signer
            .as_ref()
            .map(|(agent, secret)| (agent.as_str(), secret.as_str()))
            .unwrap_or_default();
        let (status, raw) = post_router(&self.route, &self.token, agent, secret, &body)?;
        let payload: Value = serde_json::from_str(&raw)
            .map_err(|e| format!("model router HTTP {status} returned invalid JSON: {e}"))?;
        if !(200..300).contains(&status) {
            return Err(router_refusal(status, &payload, &raw));
        }
        let content = payload
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or("the model router returned no assistant content")?;
        let answer: Value = serde_json::from_str(content)
            .map_err(|e| format!("model returned invalid assessment JSON: {e}"))?;
        Ok(
            json!({"answer": answer, "modelRequested": self.name, "modelReturned": payload.get("model"),
            "usage": payload.get("usage"), "requestSha256": digest,
            "responseSha256": crate::evidence::sha256_bytes(raw.as_bytes())}),
        )
    }
}
