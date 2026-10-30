//! The model router a talk evaluation asks, and the identity it asks with.

use super::*;

/// The model router this evaluation asks, and the identity it asks with.
pub(crate) struct TalkRouter {
    pub(crate) model: String,
    pub(crate) url: String,
    pub(crate) token: String,
    pub(crate) agent_id: String,
    pub(crate) agent_secret: String,
}

impl TalkRouter {
    pub(crate) fn resolve(
        model: Option<&str>,
        router_url: Option<&str>,
        router_bearer: Option<&str>,
        agent_id: Option<&str>,
        agent_secret: Option<&str>,
    ) -> Result<Self, Failure> {
        let route = ModelRoute::select(setting(router_url, "STADO_MODEL_ROUTER_URL")?.as_deref())
            .map_err(|detail| Failure::config("evaluate.talk", detail))?;
        let model = route
            .model(setting(model, "PROBIERZ_TALK_MODEL")?)
            .ok_or_else(|| {
                Failure::config(
                    "evaluate.talk",
                    "--model or PROBIERZ_TALK_MODEL names the vision model that judges the talk",
                )
            })?;
        let token = route
            .bearer(setting(router_bearer, "STADO_MODEL_ROUTER_TOKEN")?)
            .ok_or_else(|| {
                Failure::config(
                    "evaluate.talk",
                    "STADO_MODEL_ROUTER_TOKEN or --router-token-stdin gives the router bearer",
                )
            })?;
        let agent_id = setting(agent_id, "PROBIERZ_MODEL_AGENT_ID")?;
        let agent_secret = setting(agent_secret, "PROBIERZ_MODEL_AGENT_SECRET")?;
        let (agent_id, agent_secret) = match (agent_id, agent_secret) {
            // An endpoint Stado hands out in Brama's place reads no signature.
            _ if !route.signs() => (String::new(), String::new()),
            (Some(id), Some(secret)) => (id, secret),
            (None, None) => (String::new(), String::new()),
            _ => {
                return Err(Failure::config(
                    "evaluate.talk",
                    "agent identity needs both an agent ID and an agent secret",
                ))
            }
        };
        Ok(Self {
            model,
            url: route.url,
            token,
            agent_id,
            agent_secret,
        })
    }
}

/// The value given on the command line, else the one `name` holds in the
/// environment; absent when neither states one. A variable that is set but
/// not Unicode is refused naming it.
fn setting(explicit: Option<&str>, name: &str) -> Result<Option<String>, Failure> {
    let raw = match explicit.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value.to_string(),
        None => match std::env::var(name) {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(Failure::config(
                    "evaluate.talk",
                    format!("{name} is not valid Unicode"),
                ))
            }
        },
    };
    let value = raw.trim().to_string();
    Ok(if value.is_empty() { None } else { Some(value) })
}
