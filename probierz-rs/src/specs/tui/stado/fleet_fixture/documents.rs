use serde_json::json;
use super::*;
pub(crate) const FIXTURE_TRUSTED_KEY: &str = "nLCK4gGkYVMcTdVBFTtDMuHrX2W0EMMTNXZ3F8DGKgQ=";

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

