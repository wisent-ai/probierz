//! Where the object store is, what may be said to it, and which URIs this
//! product is allowed to address.
//!
//! Split out of `objects/mod.rs`, which had grown past three hundred lines.
//! The operations that read and delete objects stay there; everything about
//! addressing and credentials is here.

use crate::evidence::*;

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

/// The loopback Stado API every fleet host serves; a host runs its own object
/// store behind it.
const LOCAL_STADO_API: &str = "http://127.0.0.1:18776";
/// Where the fleet keeps this product's object-store token.
const OBJECT_API_ITEM: &str = "probierz-object-api";
const OBJECT_API_FIELD: &str = "token";

/// Read the object-store token the fleet holds for Probierz.
///
/// A job on a fleet host is handed `STADO_API_TOKEN`; an operator running the
/// same command from a terminal is not, and until 2026-09-21 every such run
/// refused with `STADO_API_TOKEN is required for remote object storage` — a
/// sentence about a variable rather than about the credential the fleet
/// already holds for this product. Reading it through Stado is the path
/// every other Probierz call to the fleet takes.
fn token_from_vault() -> Result<String, Failure> {
    let output = std::process::Command::new(crate::stado::STADO_BIN)
        .args([
            "credentials",
            "get",
            OBJECT_API_ITEM,
            "--field",
            OBJECT_API_FIELD,
        ])
        .output()
        .map_err(|error| {
            Failure::config(
                "objects.config",
                format!(
                    "cannot run {} to read {OBJECT_API_ITEM}: {error}",
                    crate::stado::STADO_BIN
                ),
            )
        })?;
    if !output.status.success() {
        return Err(Failure::config(
            "objects.config",
            format!(
                "Stado refused to read {OBJECT_API_ITEM} field {OBJECT_API_FIELD}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err(Failure::config(
            "objects.config",
            format!("{OBJECT_API_ITEM} field {OBJECT_API_FIELD} holds nothing in the vault"),
        ));
    }
    Ok(token)
}

pub(crate) fn object_store_config() -> Result<(String, String), Failure> {
    let mut raw = std::env::var("STADO_API_URL").unwrap_or_default();
    let mut token = std::env::var("STADO_API_TOKEN").unwrap_or_default();
    if raw.is_empty() {
        raw = LOCAL_STADO_API.to_string();
    }
    if token.is_empty() {
        token = token_from_vault()?;
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
    // Capacity readings and the evidence a fleet run leaves behind. The
    // second is what retention has to reach: the results of runs this
    // harness dispatched to the fleet accumulate in the store on the host
    // that ran them, and on charless-mac-mini they had grown to 34.9 GiB
    // with nothing able to expire them.
    // A listing addresses the root itself, a read addresses one object under
    // it, so both the bare prefix and a key below it are accepted.
    let under_root = |root: &str| key == root || key.starts_with(&format!("{root}/"));
    if !under_root("capacity") && !under_root("results") {
        return Err(Failure::invalid(
            "objects.uri",
            "Stado object URI must stay under stado://probierz/capacity/ or stado://probierz/results/",
        ));
    }
    Ok(("probierz".into(), key))
}
