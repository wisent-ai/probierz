//! Brama, or the endpoint Stado's capability resolution hands out in its
//! place, plans declared checks and explains observations; it never executes
//! code.
use crate::authoring::{hmac_sha256, required_setting, router_refusal, ModelRoute};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Model {
    url: String,
    token: String,
    /// The agent id and secret Brama's requests are signed with; none for an
    /// endpoint in Brama's place, which reads no signature.
    signer: Option<(String, String)>,
    pub name: String,
    client: reqwest::blocking::Client,
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
            return Err("--model must name a Brama route".into());
        }
        let route = ModelRoute::select(setting(url, "STADO_MODEL_ROUTER_URL")?.as_deref())?;
        let url = route.url.clone();
        // Use URL parsing rather than the legacy string-prefix loopback test.
        let parsed = url::Url::parse(&url).map_err(|e| format!("model router URL: {e}"))?;
        let loopback = match parsed.host().ok_or("model router URL has no host")? {
            url::Host::Ipv4(address) => address.is_loopback(),
            url::Host::Ipv6(address) => address.is_loopback(),
            url::Host::Domain(domain) => domain == "localhost",
        };
        if parsed.scheme() != "https" && !loopback {
            return Err(
                "model router URL requires HTTPS except literal loopback or localhost".into(),
            );
        }
        let token = required_setting(
            route.bearer(setting(bearer, "STADO_MODEL_ROUTER_TOKEN")?),
            "STADO_MODEL_ROUTER_TOKEN",
        )?;
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
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(None)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            url,
            token,
            signer,
            name: name.into(),
            client,
        })
    }
    pub fn ask(&self, instruction: &str, data: &Value) -> Result<Value, String> {
        let body = json!({"model": self.name, "messages": [
            {"role": "system", "content": instruction}, {"role": "user", "content": data.to_string()}
        ]}).to_string();
        let digest = crate::evidence::sha256_bytes(body.as_bytes());
        // The request's causes are text, which stado_wait::blocking cannot carry
        // (it settles a std::error::Error), so the wait is begun and settled here.
        let waiting = stado_wait::begin(
            stado_wait::Kind::Network,
            "model security assessment",
            &self.url,
        );
        let assessed = (|| -> Result<Value, String> {
            let mut request = self
                .client
                .post(format!("{}/v1/chat/completions", self.url))
                .bearer_auth(&self.token)
                .header("content-type", "application/json");
            if let Some((agent, secret)) = &self.signer {
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_secs()
                    .to_string();
                let signature = hmac_sha256(
                    secret.as_bytes(),
                    format!("{agent}:{timestamp}:{digest}").as_bytes(),
                )?;
                request = request
                    .header("x-agent-id", agent)
                    .header("x-agent-timestamp", timestamp)
                    .header("x-agent-signature", signature);
            }
            let response = request
                .body(body)
                .send()
                .map_err(|e| format!("model router request to {} failed: {e}", self.url))?;
            let status = response.status();
            let raw = response
                .text()
                .map_err(|e| format!("model router response HTTP {status}: {e}"))?;
            let payload: Value = serde_json::from_str(&raw)
                .map_err(|e| format!("model router HTTP {status} returned invalid JSON: {e}"))?;
            if !status.is_success() {
                return Err(router_refusal(status.as_u16(), &payload, &raw));
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
        })();
        match &assessed {
            Ok(_) => waiting.done(),
            Err(cause) => waiting.failed(cause),
        }
        assessed
    }
}
