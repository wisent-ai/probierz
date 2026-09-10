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
    pub platform: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<&'static str>,
    #[serde(rename = "apiUrl", skip_serializing_if = "Option::is_none")]
    pub api_url: Option<&'static str>,
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
            api_url: None,
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
/// so `stado:mini` still submits through Stado's configured address.
pub fn host_inventory() -> Vec<Host> {
    let dedicated = |host: &str,
                     target: &'static str,
                     api_url: Option<&'static str>,
                     pinned: &str,
                     description: &'static str| Host {
        host: host.to_string(),
        kind: "stado",
        platform: Some("darwin"),
        target: Some(target),
        api_url,
        request: Some(serde_json::json!({
            "provider": "local",
            "pin_to_provider": true,
            "pinned_host": pinned,
        })),
        description,
    };
    vec![
        Host {
            host: "local".to_string(),
            kind: "local",
            platform: None,
            target: None,
            api_url: None,
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
        dedicated(
            "stado:mini",
            "charless-mac-mini",
            None,
            "local-charless-mac-mini.local",
            "stado queue, dedicated Mac mini consumer",
        ),
        dedicated(
            "stado:macbook",
            "lukasz-macbook",
            Some("http://127.0.0.1:18765"),
            "local-lukaszs-macbook-pro-5485.local",
            "stado queue, dedicated MacBook consumer",
        ),
        Host::stado(
            "stado:t4",
            serde_json::json!({ "gpu_type": "nvidia-tesla-t4" }),
            "stado queue, nvidia-tesla-t4 capacity",
        ),
    ]
}

/// One host by its selector, or nothing when the selector is unknown.
pub fn stado_host(name: &str) -> Option<Host> {
    host_inventory()
        .into_iter()
        .find(|entry| entry.host == name)
}

pub fn hosts() -> Answer {
    print_json(&host_inventory())
}

/// Where a spec path resolves for a surface, used by run and author paths.
pub fn spec_dir(surface: &str) -> Option<PathBuf> {
    match surface {
        "web" => Some(PathBuf::from("packages/web/tests")),
        "electron" => Some(PathBuf::from("packages/electron/tests")),
        "mobile:ios" | "mobile:android" => Some(PathBuf::from("packages/mobile/test/specs")),
        "desktop:mac" | "desktop:win" => Some(PathBuf::from("packages/desktop-native/test/specs")),
        // `tui` and `desktop:cua` journeys are functions in this crate, listed
        // from the registry, so no directory holds them.
        "desktop:cua" | "tui" => None,
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
