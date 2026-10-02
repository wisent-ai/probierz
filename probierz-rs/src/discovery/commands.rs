use crate::discovery::*;

#[derive(Debug, Serialize)]
pub(crate) struct RunCommand {
    pub(crate) target: String,
    pub(crate) command: String,
    pub(crate) note: &'static str,
}

pub fn cmd(_harness: &Path, target: &str) -> Answer {
    let Some((_, command)) = RUN_COMMANDS.iter().find(|(name, _)| *name == target) else {
        let known = RUN_COMMANDS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Failure::invalid(
            "discovery.cmd",
            format!("unknown target: {target} (one of {known})"),
        ));
    };
    print_json(&RunCommand {
        target: target.to_string(),
        command: (*command).to_string(),
        note: "read-only: this is the command to run yourself; probierz never executes it",
    })
}

/// One host a run can be placed on. The bridge reads the same inventory this
/// prints, so a selector cannot mean one thing to `hosts` and another to a
/// submission.
#[derive(Debug, Clone, Serialize)]
pub struct Host {
    pub host: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<serde_json::Value>,
    pub description: &'static str,
}

impl Host {
    pub(crate) fn stado(host: &str, request: serde_json::Value, description: &'static str) -> Self {
        Self {
            host: host.to_string(),
            kind: "stado",
            platform: None,
            target: None,
            request: Some(request),
            description,
        }
    }
}

/// The hosts a run can be placed on, in the order an operator reads them.
///
/// One inventory, read by `hosts` and by the Stado bridge: a selector cannot
/// mean one placement when it is printed and another when it is submitted. A
/// selector constrains placement only — it never replaces the queue endpoint,
/// so `stado:<target>` still submits through Stado's configured address.
pub fn host_inventory() -> Vec<Host> {
    let mut hosts = placement_hosts();
    hosts.extend(registry_hosts());
    hosts
}

/// One selector per registry host that runs a local Stado consumer:
/// `stado:<target>` pins a run to that host. Stado's registry is asked each
/// time, so a host that is added, renamed or retired is a selector the moment
/// Stado knows it, and this file names no machine. A registry that cannot be
/// read is said on standard error and leaves only the placement selectors.
fn registry_hosts() -> Vec<Host> {
    let answer = std::process::Command::new("stado")
        .args(["registry", "pull"])
        .output();
    let document = match answer {
        Ok(output) if output.status.success() => {
            serde_json::from_slice::<serde_json::Value>(&output.stdout).map_err(|error| error.to_string())
        }
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(error) => Err(error.to_string()),
    };
    let document = match document {
        Ok(document) => document,
        Err(reason) => {
            eprintln!("probierz: the Stado registry is unreadable, so no registry host is listed: {reason}");
            return Vec::new();
        }
    };
    let text = |value: &serde_json::Value, key: &str| value.get(key).and_then(serde_json::Value::as_str).map(str::to_string);
    document
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|target| text(target, "kind").as_deref() == Some("local"))
        .filter_map(|target| {
            let name = text(target, "name")?;
            let hostname = target.get("hostnames")?.as_array()?.first()?.as_str()?.to_string();
            Some(Host {
                host: format!("stado:{name}"),
                kind: "stado",
                platform: text(target, "release_platform")
                    .and_then(|platform| platform.split('-').next().map(str::to_string)),
                target: Some(name),
                request: Some(serde_json::json!({
                    "provider": "local",
                    "pin_to_provider": true,
                    "pinned_host": format!("local-{hostname}"),
                })),
                description: "stado queue, pinned to this registry host's local consumer",
            })
        })
        .collect()
}

/// The two selectors that need no registry: this machine, and Stado's
/// queue with no constraint. Constraints are written by the operator as
/// `stado?key=value&key=value`; no provider, cost cap or GPU model is built in.
fn placement_hosts() -> Vec<Host> {
    vec![
        Host {
            host: "local".to_string(),
            kind: "local",
            platform: None,
            target: None,
            request: None,
            description: "this machine",
        },
        Host::stado(
            "stado",
            serde_json::json!({}),
            "stado queue, any consumer with capacity; append ?key=value&... to constrain the placement request",
        ),
    ]
}

/// `stado?key=value&...`: every pair becomes a field of the placement
/// request. A value that reads as a number or as `true`/`false` is sent as
/// one; anything else is sent as text. The selector is what the operator
/// wrote, so `probierz hosts` and a submission read the same constraint.
fn constrained_stado_host(name: &str) -> Option<Host> {
    let query = name.strip_prefix("stado?")?;
    let mut request = serde_json::Map::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=')?;
        if key.is_empty() {
            return None;
        }
        let value = match value {
            "true" => serde_json::Value::Bool(true),
            "false" => serde_json::Value::Bool(false),
            other => match other.parse::<f64>() {
                Ok(number) => serde_json::json!(number),
                Err(_) => serde_json::Value::String(other.to_string()),
            },
        };
        request.insert(key.to_string(), value);
    }
    if request.is_empty() {
        return None;
    }
    Some(Host::stado(
        name,
        serde_json::Value::Object(request),
        "stado queue, constrained by the selector's query",
    ))
}

/// One host by its selector, or nothing when the selector is unknown. `local`,
/// `stado` and `stado?...` are answered without asking the registry;
/// `stado:<target>` is answered by it.
pub fn stado_host(name: &str) -> Option<Host> {
    placement_hosts()
        .into_iter()
        .find(|entry| entry.host == name)
        .or_else(|| constrained_stado_host(name))
        .or_else(|| registry_hosts().into_iter().find(|entry| entry.host == name))
}

pub fn hosts() -> Answer {
    print_json(&host_inventory())
}

/// Where a spec path resolves for a surface, used by run and author paths.
pub fn spec_dir(surface: &str) -> Option<PathBuf> {
    match surface {
        "mobile:ios" | "mobile:android" => Some(PathBuf::from("packages/mobile/test/specs")),
        "desktop:mac" | "desktop:win" => Some(PathBuf::from("packages/desktop-native/test/specs")),
        // `tui`, `desktop:cua` and `web` journeys are functions in this crate,
        // listed from the registry, so no directory holds them.
        "desktop:cua" | "tui" | "web" => None,
        _ => None,
    }
}

