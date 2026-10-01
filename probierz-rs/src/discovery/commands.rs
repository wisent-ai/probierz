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

/// The selectors that constrain placement without naming a host.
fn placement_hosts() -> Vec<Host> {
    vec![
        Host {
            host: "local".to_string(),
            kind: "local",
            platform: None,
            target: None,
            request: None,
            description: "this machine (default)",
        },
        Host::stado(
            "stado:gcp",
            serde_json::json!({ "provider": "gcp", "pin_to_provider": true }),
            "stado queue, GCP consumers only",
        ),
        Host::stado(
            "stado:azure",
            serde_json::json!({ "provider": "azure", "pin_to_provider": true }),
            "stado queue, Azure consumers only",
        ),
        Host::stado(
            "stado:aws",
            serde_json::json!({ "provider": "aws", "pin_to_provider": true }),
            "stado queue, AWS consumers only",
        ),
        Host::stado(
            "stado:any",
            serde_json::json!({}),
            "stado queue, any consumer with capacity",
        ),
        Host::stado(
            "stado:spot",
            serde_json::json!({ "max_cost_per_hour_usd": 4 }),
            "stado queue, cost-capped capacity",
        ),
        Host::stado(
            "stado:local",
            serde_json::json!({ "provider": "local", "pin_to_provider": true }),
            "stado queue, local-kind consumers only",
        ),
        Host::stado(
            "stado:t4",
            serde_json::json!({ "gpu_type": "nvidia-tesla-t4" }),
            "stado queue, nvidia-tesla-t4 capacity",
        ),
    ]
}

/// One host by its selector, or nothing when the selector is unknown. A
/// placement selector is answered without asking the registry.
pub fn stado_host(name: &str) -> Option<Host> {
    placement_hosts()
        .into_iter()
        .find(|entry| entry.host == name)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outline_keeps_a_quotation_mark_inside_a_title() {
        let source =
            "describe('a product\\'s journey', () => { it(\"reads `state`\", () => {}); });";
        let outline = outline_of(source);
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].kind, "describe");
        assert_eq!(outline[0].title, "a product\\'s journey");
        assert_eq!(outline[1].kind, "it");
        assert_eq!(outline[1].title, "reads `state`");
    }

    #[test]
    fn an_identifier_that_merely_starts_like_a_title_call_is_not_one() {
        let outline = outline_of("const describeLater = 1; itemCount('x'); object.it('y');");
        assert!(outline.is_empty(), "unexpected outline: {outline:?}");
    }
}
