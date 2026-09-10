use crate::authoring::*;
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
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
}

pub(crate) fn loopback(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "::1"
        || host == "[::1]"
        || host.starts_with("127.")
}

pub(crate) fn stado_model_router_url(raw: Option<&str>) -> Result<String, String> {
    let configured = raw.unwrap_or_default().trim();
    if configured.is_empty() {
        return Err("STADO_MODEL_ROUTER_URL is required".to_string());
    }
    let (scheme, remainder) = configured
        .split_once("://")
        .ok_or_else(|| "STADO_MODEL_ROUTER_URL must be a valid URL".to_string())?;
    if remainder.is_empty() || remainder.chars().any(char::is_whitespace) {
        return Err("STADO_MODEL_ROUTER_URL must be a valid URL".to_string());
    }
    if remainder.contains('@') || remainder.contains('?') || remainder.contains('#') {
        return Err(
            "STADO_MODEL_ROUTER_URL must not contain credentials, query parameters, or a fragment"
                .to_string(),
        );
    }
    let authority = remainder.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err("STADO_MODEL_ROUTER_URL must be a valid URL".to_string());
    }
    let host = if authority.starts_with('[') {
        authority
            .split_once(']')
            .map(|(value, _)| format!("{value}]"))
            .unwrap_or_else(|| authority.to_string())
    } else {
        authority.split(':').next().unwrap_or_default().to_string()
    };
    if scheme != "https" && !(scheme == "http" && loopback(&host)) {
        return Err("STADO_MODEL_ROUTER_URL must use HTTPS or loopback HTTP".to_string());
    }
    Ok(configured.trim_end_matches('/').to_string())
}

pub(crate) fn required_setting(value: Option<String>, name: &str) -> Result<String, String> {
    value
        .filter(|item| !item.trim().is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

pub(crate) fn hmac_sha256(secret: &[u8], message: &[u8]) -> String {
    let mut key = [0u8; 64];
    if secret.len() > 64 {
        key[..32].copy_from_slice(&Sha256::digest(secret));
    } else {
        key[..secret.len()].copy_from_slice(secret);
    }
    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for index in 0..64 {
        inner_pad[index] ^= key[index];
        outer_pad[index] ^= key[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner);
    hex::encode(outer.finalize())
}

pub(crate) struct RouterReply {
    pub(crate) content: String,
    pub(crate) model: JsonValue,
    pub(crate) usage: JsonValue,
}

pub(crate) fn temp_file(label: &str, content: &[u8]) -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let file =
        std::env::temp_dir().join(format!("probierz-{label}-{}-{stamp}", std::process::id()));
    let mut handle = File::create(&file).map_err(|error| error.to_string())?;
    handle
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    handle
        .write_all(content)
        .map_err(|error| error.to_string())?;
    Ok(file)
}

pub(crate) fn post_router(
    url: &str,
    token: &str,
    agent_id: &str,
    agent_secret: &str,
    body: &str,
    budget_seconds: u64,
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
        let digest = hex::encode(Sha256::digest(body.as_bytes()));
        let signature = hmac_sha256(
            agent_secret.as_bytes(),
            format!("{agent_id}:{timestamp}:{digest}").as_bytes(),
        );
        headers.push_str(&format!("x-agent-id: {agent_id}\nx-agent-timestamp: {timestamp}\nx-agent-signature: {signature}\n"));
    }
    let header_file = temp_file("router-headers", headers.as_bytes())?;
    let body_file = temp_file("router-body", body.as_bytes())?;
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            &budget_seconds.to_string(),
            "--header",
        ])
        .arg(format!("@{}", header_file.display()))
        .args(["--data-binary"])
        .arg(format!("@{}", body_file.display()))
        .args([
            "--write-out",
            "\n%{http_code}",
            &format!("{url}/v1/chat/completions"),
        ])
        .output();
    let _ = fs::remove_file(&header_file);
    let _ = fs::remove_file(&body_file);
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
        .map_err(|_| "model router returned an invalid HTTP status".to_string())?;
    Ok((status, payload.to_string()))
}

