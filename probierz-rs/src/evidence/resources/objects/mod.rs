//! The two object-store operations this product performs, and the endpoint
//! rules they obey.

mod endpoint;

pub(crate) use endpoint::{object_store_config, split_object_uri};

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

/// Remove one object this product owns.
///
/// The same `DELETE /api/object` Stado's own CLI performs, through the grant
/// the fleet holds for Probierz. Only the prefixes `split_object_uri`
/// accepts can be addressed, so a retention pass can reach a run's evidence
/// and nothing else.
pub fn remove_object(uri: &str) -> Result<(), Failure> {
    split_object_uri(uri)?;
    let (base_url, token) = object_store_config()?;
    let mut url = Url::parse(&format!("{base_url}/api/object"))
        .map_err(|error| Failure::config("objects.config", error.to_string()))?;
    url.query_pairs_mut().append_pair("uri", uri);
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let response = match agent
        .delete(url.as_str())
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _)) => {
            return Err(Failure::unavailable(
                "objects.delete",
                format!("Stado object storage refused to remove {uri}: {status}"),
            ));
        }
        Err(error) => {
            return Err(Failure::unavailable(
                "objects.delete",
                format!("Stado object storage did not answer the removal of {uri}: {error}"),
            ));
        }
    };
    let payload: Value = response
        .into_json()
        .map_err(|error| Failure::unavailable("objects.delete", error.to_string()))?;
    if payload.get("state").and_then(Value::as_str) != Some("absent")
        || payload.get("uri").and_then(Value::as_str) != Some(uri)
    {
        return Err(Failure::unavailable(
            "objects.delete",
            format!("Stado object storage did not report {uri} absent after removing it"),
        ));
    }
    Ok(())
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
