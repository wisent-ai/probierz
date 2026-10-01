use crate::stado::*;
use serde_json::json;

pub(crate) fn manifest_string<'a>(
    document: &'a serde_yaml::Value,
    path: &[&str],
) -> Option<&'a str> {
    let mut current = document;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str()
}

pub(crate) fn remote_secret_env(
    harness: &Path,
    app_id: &str,
    names: &[&str],
) -> Result<Value, Failure> {
    let application = manifest::load(harness, app_id)?;
    let configured = application
        .document
        .get("secretRefs")
        .and_then(serde_yaml::Value::as_mapping);
    let mut answer = Map::new();
    for name in names {
        let (reference, item, field) = match *name {
            "STADO_MODEL_ROUTER_TOKEN" => {
                (MODEL_ROUTER_REFERENCE, "probierz-model-router", "token")
            }
            "PROBIERZ_MODEL_AGENT_SECRET" => (
                MODEL_AGENT_REFERENCE,
                "probierz-agent-auth",
                "agent_auth_secret",
            ),
            "PROBIERZ_SEO_RECEIPT_PRIVATE_KEY" => (
                SEO_KEY_REFERENCE,
                "probierz-seo-receipt-signing",
                "private_key",
            ),
            _ => continue,
        };
        let found = configured
            .and_then(|mapping| mapping.get(serde_yaml::Value::from(*name)))
            .and_then(serde_yaml::Value::as_str);
        let Some(found) = found else {
            continue;
        };
        if found != reference {
            return Err(Failure::config(
                "stado.submit",
                format!("Remote runs require {reference} for {name}."),
            ));
        }
        answer.insert((*name).to_string(), json!({ "item": item, "field": field }));
    }
    Ok(Value::Object(answer))
}
