use crate::evidence::*;
pub fn list_objects(root_uri: &str) -> Result<Vec<Value>, Failure> {
    let (namespace, key) = split_object_uri(root_uri)?;
    let (base_url, token) = object_store_config()?;
    let mut url = Url::parse(&format!("{base_url}/api/object/list"))
        .map_err(|error| Failure::config("objects.config", error.to_string()))?;
    url.query_pairs_mut()
        .append_pair("namespace", &namespace)
        .append_pair("prefix", &key);
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let response = match agent
        .get(url.as_str())
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _)) => {
            return Err(Failure::unavailable(
                "objects.read",
                format!("Stado object storage rejected the request: {status}"),
            ));
        }
        Err(error) => {
            return Err(Failure::unavailable(
                "objects.read",
                format!("Stado object storage did not answer: {error}"),
            ));
        }
    };
    let payload: Value = response
        .into_json()
        .map_err(|error| Failure::unavailable("objects.list", error.to_string()))?;
    let objects = payload
        .get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Failure::unavailable("objects.list", "Stado list response has no objects array")
        })?;
    for item in objects {
        let uri = item.get("uri").and_then(Value::as_str).ok_or_else(|| {
            Failure::unavailable(
                "objects.list",
                "Stado list response contains an invalid object",
            )
        })?;
        let item_key = item.get("key").and_then(Value::as_str).ok_or_else(|| {
            Failure::unavailable(
                "objects.list",
                "Stado list response contains an invalid object",
            )
        })?;
        let (listed_namespace, listed_key) = split_object_uri(uri)?;
        if listed_namespace != namespace
            || listed_key != item_key
            || (listed_key != key && !listed_key.starts_with(&format!("{key}/")))
        {
            return Err(Failure::unavailable(
                "objects.list",
                "Stado list response escaped the requested Probierz prefix",
            ));
        }
    }
    Ok(objects.clone())
}

pub(crate) fn unsafe_url_text(value: &str) -> bool {
    value.trim() != value
        || value
            .chars()
            .any(|character| character <= '\u{1f}' || character == '\u{7f}')
        || value.contains(['\\', '%'])
        || value.split('/').any(|part| matches!(part, "." | ".."))
}

pub(crate) fn loopback(host: &str) -> bool {
    host == "localhost" || host == "::1" || host == "[::1]" || {
        let parts = host.split('.').collect::<Vec<_>>();
        parts.len() == 4
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 3
                    && part.bytes().all(|byte| byte.is_ascii_digit())
                    && part.parse::<u8>().is_ok()
            })
            && parts[0] == "127"
    }
}

pub(crate) fn object_store_config() -> Result<(String, String), Failure> {
    let raw = std::env::var("STADO_API_URL").unwrap_or_default();
    let token = std::env::var("STADO_API_TOKEN").unwrap_or_default();
    if raw.is_empty() {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_URL is required for remote object storage",
        ));
    }
    if token.is_empty() {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_TOKEN is required for remote object storage",
        ));
    }
    if unsafe_url_text(&raw) {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_URL contains unsafe URL syntax",
        ));
    }
    if token
        .chars()
        .any(|character| character <= '\u{1f}' || character == '\u{7f}')
    {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_TOKEN contains control characters",
        ));
    }
    let parsed = Url::parse(&raw).map_err(|_| {
        Failure::config(
            "objects.config",
            "STADO_API_URL must be an absolute HTTP(S) URL",
        )
    })?;
    let unsafe_base = !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path().contains("//")
        || (parsed.path() != "/" && parsed.path().ends_with('/'));
    if unsafe_base {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_URL must not contain credentials, query, fragment, or an unsafe base path",
        ));
    }
    if parsed.scheme() == "http" && !parsed.host_str().is_some_and(loopback) {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_URL must use HTTPS except for authenticated loopback",
        ));
    }
    let origin = parsed.origin().ascii_serialization();
    Ok((
        format!(
            "{origin}{}",
            if parsed.path() == "/" {
                ""
            } else {
                parsed.path()
            }
        ),
        token,
    ))
}

pub(crate) fn split_object_uri(uri: &str) -> Result<(String, String), Failure> {
    if uri.is_empty() || unsafe_url_text(uri) {
        return Err(Failure::invalid(
            "objects.uri",
            format!("unsafe Stado object URI: {uri}"),
        ));
    }
    let parsed = Url::parse(uri)
        .map_err(|_| Failure::invalid("objects.uri", format!("invalid Stado object URI: {uri}")))?;
    if parsed.scheme() != "stado"
        || parsed.host_str() != Some("probierz")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.path().starts_with('/')
        || parsed.path().starts_with("//")
        || parsed.path().contains("//")
    {
        return Err(Failure::invalid(
            "objects.uri",
            format!("invalid Stado object URI: {uri}"),
        ));
    }
    let key = parsed.path().trim_start_matches('/').to_string();
    if !key.starts_with("capacity/") {
        return Err(Failure::invalid(
            "objects.uri",
            "Stado object URI must stay under stado://probierz/capacity/",
        ));
    }
    Ok(("probierz".into(), key))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;

    #[test]
    fn canonical_json_sorts_every_object_level() {
        assert_eq!(
            canonical(&json!({"z": [3, {"b": true, "a": null}], "a": "x"})),
            r#"{"a":"x","z":[3,{"a":null,"b":true}]}"#
        );
    }

    #[test]
    fn resources_match_shared_driver_boundaries() {
        let env = BTreeMap::from([
            ("IOS_DEVICE".into(), "iPhone 17".into()),
            ("IOS_VERSION".into(), "26".into()),
        ]);
        assert_eq!(
            resources_for("mobile:ios", &env),
            vec!["device:ios:iPhone 17:26", "port:4723"]
        );
    }
}
