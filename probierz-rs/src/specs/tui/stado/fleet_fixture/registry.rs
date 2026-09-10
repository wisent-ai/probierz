use serde_json::json;
use super::*;
pub(crate) fn this_hostname() -> Result<String, String> {
    let output = Command::new("hostname").output().map_err(|error| {
        format!("this host has no hostname, so no fixture target can name it: {error}")
    })?;
    let hostname = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();
    if hostname.is_empty() {
        Err("this host has no hostname, so no fixture target can name it".to_string())
    } else {
        Ok(hostname)
    }
}

pub(crate) fn fixture_registry(
    target: Value,
    release_control: Option<Value>,
) -> Result<Value, String> {
    let mut fixture_target = json!({
        "name": FIXTURE_HOST,
        "kind": "local",
        "hostnames": [this_hostname()?],
        "release_platform": "darwin-arm64",
        "role": "interactive",
        "notes": "Probierz journey fixture host. This machine, scoped to a temp HOME.",
    });
    let additions = target
        .as_object()
        .ok_or_else(|| "fixture registry target must be an object".to_string())?;
    fixture_target
        .as_object_mut()
        .expect("fixture target object")
        .extend(additions.clone());
    let mut document = json!({
        "schema_version": 2,
        "targets": [fixture_target],
    });
    if let Some(release_control) = release_control {
        document["release_control"] = release_control;
    }
    Ok(document)
}

pub(crate) fn fixture_release_control(
    home: &Path,
    state_dir: &Path,
    logs_root: &Path,
    desired_version: &str,
    desired_digest: &str,
    install_root: &Path,
) -> Value {
    json!({
        "schema_version": 1,
        "generation": 1,
        "trusted_keys": { "stado-release-2026-08": FIXTURE_TRUSTED_KEY },
        "products": {
            FIXTURE_PRODUCT: {
                "service": FIXTURE_PRODUCT,
                "config_schema": 1,
                "state_schema": 1,
                "install_root": install_root,
                "binary": "bin/fixture",
                "launcher": "bin/start",
                "binary_env": "PROBIERZ_FIXTURE_BIN",
                "port_env": "PROBIERZ_FIXTURE_PORT",
                "runtime_env": "PROBIERZ_FIXTURE_RUNTIME_DIR",
                "strategy": {
                    "kind": "blue-green",
                    "readiness_timeout_seconds": 90,
                    "drain_timeout_seconds": 60,
                    "rollback_window_seconds": 300,
                    "automatic_rollback": true,
                },
                "desired": {
                    "version": desired_version,
                    "channel": "stable",
                    "rollout_generation": 2,
                    "promoted_at": "2026-08-17T09:00:00+00:00",
                    "artifacts": {
                        "darwin-arm64": {
                            "archive_uri": format!("stado://releases/{FIXTURE_PRODUCT}/{desired_version}/darwin-arm64/release.tar.gz"),
                            "artifact_sha256": desired_digest,
                            "manifest_uri": format!("stado://releases/{FIXTURE_PRODUCT}/{desired_version}/darwin-arm64/release.json"),
                            "manifest_sha256": "b".repeat(64),
                            "signature_uri": format!("stado://releases/{FIXTURE_PRODUCT}/{desired_version}/darwin-arm64/release.sig"),
                            "key_id": "stado-release-2026-08",
                            "source_revision": "c".repeat(40),
                        }
                    }
                },
                "targets": {
                    FIXTURE_HOST: {
                        "platform": "darwin-arm64",
                        "run_as_user": std::env::var("USER").unwrap_or_else(|_| "operator".to_string()),
                        "home": home,
                        "state_dir": state_dir,
                        "runtime_root": home.join(".stado/run"),
                        "logs_root": logs_root,
                        "stable_bind": "127.0.0.1:18190",
                        "candidate_ports": [18191, 18192],
                        "readiness_path": "/health",
                    }
                }
            }
        }
    })
}

pub(crate) fn settled_state(version: &str, digest: &str, release_dir: &Path) -> Value {
    let started_at = iso_timestamp(SystemTime::now() - Duration::from_secs(3_600));
    json!({
        "schema_version": 1,
        "product": FIXTURE_PRODUCT,
        "target": FIXTURE_HOST,
        "rollout_generation": 2,
        "phase": "committed",
        "active": {
            "version": version,
            "artifact_sha256": digest,
            "manifest_sha256": "b".repeat(64),
            "port": 18190,
            "pid": 1,
            "release_dir": release_dir,
            "started_at": started_at,
        },
        "previous": null,
        "candidate": null,
        "proxy_pid": null,
        "cutover_at": started_at,
        "quarantined": {},
        "detail": "",
        "updated_at": iso_timestamp(SystemTime::now() - Duration::from_secs(60)),
    })
}

pub(crate) fn source_identity() -> Result<Value, String> {
    let revision = Command::new("git")
        .args(["-C", STADO_REPO, "rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("cannot read the source revision of {STADO_REPO}: {error}"))?;
    let status = Command::new("git")
        .args(["-C", STADO_REPO, "status", "--porcelain"])
        .output()
        .map_err(|error| format!("cannot read the source state of {STADO_REPO}: {error}"))?;
    if !revision.status.success() {
        return Err(format!("cannot read the source revision of {STADO_REPO}"));
    }
    Ok(json!({
        "repository": STADO_REPO,
        "revision": String::from_utf8_lossy(&revision.stdout).trim(),
        "dirty": !String::from_utf8_lossy(&status.stdout).trim().is_empty(),
    }))
}

pub(crate) fn record_trace(
    context: &specs::Context,
    slug: &str,
    journey: &str,
    binary: &str,
    source: Value,
    observations: Value,
    contracts: &[&str],
) -> Result<(), String> {
    let trace_path = context.artifacts.join(format!("{slug}.trace.json"));
    fs::create_dir_all(&context.artifacts)
        .map_err(|error| format!("{}: {error}", context.artifacts.display()))?;
    let body = serde_json::to_vec_pretty(&json!({
        "schemaVersion": 1,
        "kind": "probierz-stado-fleet-trace",
        "journey": journey,
        "runId": context.optional("PROBIERZ_RUN_ID"),
        "status": "completed",
        "binary": binary,
        "source": source,
        "host": { "fixtureTarget": FIXTURE_HOST, "hostname": this_hostname()? },
        "productionMutations": "none: every command ran against an isolated fixture host",
        "observations": observations,
        "contracts": contracts,
        "redaction": {
            "status": "verified_redacted",
            "credentialsIncluded": false,
            "productionIdentifiersIncluded": false,
        }
    }))
    .map_err(|error| error.to_string())?;
    let mut terminated = body;
    terminated.push(b'\n');
    write_private(&trace_path, &terminated)
        .map_err(|error| format!("{}: {error}", trace_path.display()))?;
    context.media_typed("trace", trace_path, "application/json");
    Ok(())
}

