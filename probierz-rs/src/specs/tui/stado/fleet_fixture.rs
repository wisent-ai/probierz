use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{json, Value};

use crate::failure::{iso_timestamp, now_iso, write_private};
use crate::{specs, tui};

pub(crate) const STADO_REPO: &str =
    "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-compute";
pub(crate) const DEFAULT_STADO_BINARY: &str =
    "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-compute/stado-rs/target/release/stado";
pub(crate) const FIXTURE_HOST: &str = "probierz-fixture-host";
pub(crate) const FIXTURE_PRODUCT: &str = "probierz-fixture-product";
const FIXTURE_TRUSTED_KEY: &str = "nLCK4gGkYVMcTdVBFTtDMuHrX2W0EMMTNXZ3F8DGKgQ=";

pub(crate) struct Invocation {
    pub status: i32,
    pub output: String,
    pub json: Value,
}

pub(crate) struct FleetFixture {
    pub binary: String,
    pub dir: PathBuf,
    pub home: PathBuf,
    pub store: PathBuf,
    pub state_dir: PathBuf,
    pub logs_root: PathBuf,
    pub services_root: PathBuf,
    pub capture: PathBuf,
    terminal: Option<tui::Terminal>,
    slug: String,
    command_number: usize,
}

impl FleetFixture {
    pub(crate) fn open(context: &specs::Context, slug: &str) -> Result<Self, String> {
        let binary = context
            .optional("TUI_CMD")
            .unwrap_or_else(|| DEFAULT_STADO_BINARY.to_string());
        if !Path::new(&binary).exists() {
            return Err(format!("no stado binary at {binary}; build it first"));
        }

        let cache_home = std::env::var("HOME").map(PathBuf::from).map_err(|_| {
            "HOME is required to place the isolated Stado fixture cache".to_string()
        })?;
        let scratch_root = cache_home.join("Library/Caches/probierz-journeys");
        fs::create_dir_all(&scratch_root)
            .map_err(|error| format!("cannot create {}: {error}", scratch_root.display()))?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = scratch_root.join(format!("{slug}-{}-{stamp}", std::process::id()));
        fs::create_dir(&dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        let home = dir.join("home");
        let store = dir.join("store");
        let state_dir = home.join(".stado/release-state");
        let logs_root = home.join(".stado/logs");
        let services_root = home.join(".stado/services");
        let capture = dir.join("capture");
        for path in [
            &home,
            &store,
            &state_dir,
            &logs_root,
            &services_root,
            &capture,
        ] {
            fs::create_dir_all(path)
                .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        }

        let ready = format!("__PZ_{}_READY__", marker_slug(slug));
        let shell = format!("stty -echo; printf '{}\\n'; exec /bin/sh", ready);
        let terminal = tui::Terminal::spawn(
            tui::Spawn::new("/bin/sh")
                .arg("-c")
                .arg(shell)
                .cwd(&dir)
                .env("HOME", home.to_string_lossy())
                .env("WC_STORAGE_BACKEND", "local")
                .env("WC_LOCAL_STORAGE_PATH", store.to_string_lossy())
                .env(
                    "PATH",
                    "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
                )
                .size(400, 60),
        )
        .map_err(|error| error.to_string())?;
        terminal
            .wait_for(&ready, Duration::from_secs(15), true)
            .map_err(|error| error.to_string())?;

        Ok(Self {
            binary,
            dir,
            home,
            store,
            state_dir,
            logs_root,
            services_root,
            capture,
            terminal: Some(terminal),
            slug: slug.to_string(),
            command_number: 0,
        })
    }

    pub(crate) fn invoke(&mut self, args: &[&str]) -> Result<Invocation, String> {
        self.invoke_with_timeout(args, Duration::from_secs(120), false)
    }

    pub(crate) fn invoke_json(&mut self, args: &[&str]) -> Result<Invocation, String> {
        self.invoke_with_timeout(args, Duration::from_secs(120), true)
    }

    fn invoke_with_timeout(
        &mut self,
        args: &[&str],
        timeout: Duration,
        capture_json: bool,
    ) -> Result<Invocation, String> {
        self.command_number += 1;
        let marker = format!(
            "__PZ_{}_{}_DONE__",
            marker_slug(&self.slug),
            self.command_number
        );
        let terminal = self
            .terminal
            .as_mut()
            .ok_or_else(|| "the fixture terminal is closed".to_string())?;
        let log_start = terminal.full_log().len();
        let command = std::iter::once(self.binary.as_str())
            .chain(args.iter().copied())
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ");
        let payload_path = self
            .capture
            .join(format!("{}-{}.json", self.slug, self.command_number));
        let line = if capture_json {
            format!(
                "{command} > {}; printf '\\n{marker}:%s\\n' \"$?\"",
                shell_quote(payload_path.to_string_lossy().as_ref())
            )
        } else {
            format!("{command} 2>&1; printf '\\n{marker}:%s\\n' \"$?\"")
        };
        terminal.send(&line).map_err(|error| error.to_string())?;
        terminal.key("enter").map_err(|error| error.to_string())?;
        terminal
            .wait_for(&marker, timeout, true)
            .map_err(|error| error.to_string())?;
        let complete = terminal.full_log();
        let log = complete
            .get(log_start..)
            .ok_or_else(|| "terminal log changed at a non-character boundary".to_string())?;
        let status_pattern = Regex::new(&format!(r"{}:(\d+)", regex::escape(&marker)))
            .map_err(|error| error.to_string())?;
        let status = status_pattern
            .captures(log)
            .and_then(|capture| capture.get(1))
            .and_then(|value| value.as_str().parse::<i32>().ok())
            .ok_or_else(|| format!("no exit status for: stado {}", args.join(" ")))?;
        let output = log[..log.find(&marker).unwrap_or(log.len())].to_string();
        let raw = if capture_json && payload_path.exists() {
            fs::read_to_string(&payload_path)
                .map_err(|error| format!("{}: {error}", payload_path.display()))?
        } else {
            String::new()
        };
        let json = if raw.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&raw).map_err(|error| {
                format!(
                    "stado {} emitted invalid JSON: {error}\n{raw}",
                    args.join(" ")
                )
            })?
        };
        Ok(Invocation {
            status,
            output,
            json,
        })
    }

    pub(crate) fn registry(&self, document: &Value) -> Result<(), String> {
        write_json(&self.store.join("registry.json"), document)
    }

    pub(crate) fn read_registry(&self) -> Result<Value, String> {
        let path = self.store.join("registry.json");
        let text =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub(crate) fn publish_capacity(
        &self,
        diag: Value,
        available_cpu_cores: u64,
        accepting_jobs: bool,
        published_at: Option<String>,
    ) -> Result<(), String> {
        let hostname = this_hostname()?;
        write_json(
            &self.store.join(format!("capacity/local-{hostname}.json")),
            &json!({
                "consumer_id": format!("local-{hostname}"),
                "kind": "local",
                "accepting_jobs": accepting_jobs,
                "running_jobs": 0,
                "total_cpu_cores": 4,
                "available_cpu_cores": available_cpu_cores,
                "available_accelerators": {},
                "free_ram_gb": 8,
                "total_ram_gb": 16,
                "free_vram_gb": 0,
                "total_vram_gb": 0,
                "published_at": published_at.unwrap_or_else(now_iso),
                "diag": diag,
                "stado_version": "0.15.10",
            }),
        )
    }

    pub(crate) fn write_release_state(&self, state: &Value) -> Result<(), String> {
        write_json(
            &self.state_dir.join(format!("{FIXTURE_PRODUCT}.json")),
            state,
        )
    }

    pub(crate) fn close(&mut self) -> Result<(), String> {
        if let Some(terminal) = self.terminal.take() {
            terminal.close().map_err(|error| error.to_string())?;
        }
        tui::remove_scratch(&self.dir);
        Ok(())
    }
}

impl Drop for FleetFixture {
    fn drop(&mut self) {
        if let Some(terminal) = self.terminal.take() {
            let _ = terminal.close();
        }
        tui::remove_scratch(&self.dir);
    }
}

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

pub(crate) fn compile_idle_program(dir: &Path, target: &Path) -> Result<PathBuf, String> {
    let source = dir.join("idle.c");
    fs::write(
        &source,
        "#include <unistd.h>\nint main(void){for(;;){pause();}return 0;}\n",
    )
    .map_err(|error| format!("{}: {error}", source.display()))?;
    let compiled = Command::new("/usr/bin/cc")
        .args(["-O0", "-o"])
        .arg(target)
        .arg(&source)
        .output()
        .map_err(|error| format!("cannot compile the fixture service program (needs the macOS command line tools): {error}"))?;
    if !compiled.status.success() {
        return Err(format!(
            "cannot compile the fixture service program (needs the macOS command line tools): {}",
            String::from_utf8_lossy(&compiled.stderr)
        ));
    }
    set_executable(target)?;
    Ok(target.to_path_buf())
}

pub(crate) fn copy_program(from: &Path, to: &Path) -> Result<PathBuf, String> {
    fs::copy(from, to)
        .map_err(|error| format!("{} -> {}: {error}", from.display(), to.display()))?;
    set_executable(to)?;
    Ok(to.to_path_buf())
}

pub(crate) fn write_agent_plist(
    path: &Path,
    label: &str,
    program_args: &[&Path],
) -> Result<(), String> {
    let args = program_args
        .iter()
        .map(|argument| format!("        <string>{}</string>", argument.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n    <key>Label</key>\n    <string>{label}</string>\n    <key>ProgramArguments</key>\n    <array>\n{args}\n    </array>\n    <key>RunAtLoad</key>\n    <true/>\n</dict>\n</plist>\n"
    );
    fs::write(path, plist).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn bootstrap_agent(plist_path: &Path, label: &str) -> Result<u32, String> {
    let domain = launchd_domain()?;
    let loaded = Command::new("/bin/launchctl")
        .args(["bootstrap", &domain])
        .arg(plist_path)
        .output()
        .map_err(|error| format!("launchd refused the fixture job: {error}"))?;
    if !loaded.status.success() {
        return Err(format!(
            "launchd refused the fixture job: {}{}",
            String::from_utf8_lossy(&loaded.stderr),
            String::from_utf8_lossy(&loaded.stdout)
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(pid) = launchd_pid(label)? {
            return Ok(pid);
        }
        thread::sleep(Duration::from_millis(300));
    }
    Err(format!("launchd started no process for {label}"))
}

pub(crate) fn launchd_pid(label: &str) -> Result<Option<u32>, String> {
    let domain = launchd_domain()?;
    let printed = Command::new("/bin/launchctl")
        .args(["print", &format!("{domain}/{label}")])
        .output()
        .map_err(|error| format!("cannot inspect launchd job {label}: {error}"))?;
    if !printed.status.success() {
        return Ok(None);
    }
    let pid = String::from_utf8_lossy(&printed.stdout)
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("pid = "))
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0);
    Ok(pid)
}

pub(crate) fn bootout_agent(label: &str) {
    if let Ok(domain) = launchd_domain() {
        let _ = Command::new("/bin/launchctl")
            .args(["bootout", &format!("{domain}/{label}")])
            .output();
    }
}

pub(crate) fn spawn_orphan(command: &str) -> Result<u32, String> {
    let spawned = Command::new("/bin/sh")
        .args(["-c", &format!("nohup {command} >/dev/null 2>&1 & echo $!")])
        .output()
        .map_err(|error| format!("could not start the unowned fixture process: {error}"))?;
    let pid = String::from_utf8_lossy(&spawned.stdout)
        .trim()
        .parse::<u32>()
        .unwrap_or(0);
    if pid == 0 {
        Err(format!(
            "could not start the unowned fixture process: {}",
            String::from_utf8_lossy(&spawned.stderr)
        ))
    } else {
        Ok(pid)
    }
}

pub(crate) fn alive(pid: u32) -> bool {
    Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) fn stop(pid: Option<u32>) {
    if let Some(pid) = pid {
        let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
    }
}

pub(crate) fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut body = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    body.push(b'\n');
    write_private(path, &body).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn ensure(condition: bool, reason: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(reason.into())
    }
}

pub(crate) fn array_contains_string(value: &Value, needle: &str) -> bool {
    value
        .as_array()
        .map(|items| items.iter().any(|item| item.as_str() == Some(needle)))
        .unwrap_or(false)
}
pub(crate) fn value_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn marker_slug(slug: &str) -> String {
    slug.to_uppercase().replace('-', "_")
}

fn launchd_domain() -> Result<String, String> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|error| format!("cannot determine launchd domain: {error}"))?;
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !uid.is_empty() {
        Ok(format!("gui/{uid}"))
    } else {
        Err("cannot determine launchd domain".to_string())
    }
}

fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}
