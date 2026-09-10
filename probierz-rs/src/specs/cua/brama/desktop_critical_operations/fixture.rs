use super::*;
pub(crate) struct Fixture {
    pub(crate) state_root: PathBuf,
    pub(crate) routes: PathBuf,
    pub(crate) namespace: String,
    pub(crate) provider_service: String,
    pub(crate) runtime_service: String,
    pub(crate) runtime_origin: String,
    pub(crate) provider: &'static str,
    pub(crate) first_key: String,
    pub(crate) replacement_key: String,
    pub(crate) alias: String,
}

impl Fixture {
    pub(crate) fn new(context: &specs::Context) -> Result<Self, String> {
        let suffix = common::unique_suffix();
        let state_root = context
            .artifacts
            .join(format!("{}-state-{suffix}", context.title));
        let namespace = format!("ai.wisent.brama.desktop.probierz.{suffix}");
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("could not reserve a local runtime port: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        drop(listener);
        Ok(Self {
            routes: state_root.join("Runtime/routes.json"),
            state_root,
            provider_service: format!("{namespace}.providers"),
            runtime_service: format!("{namespace}.runtime"),
            runtime_origin: format!("http://127.0.0.1:{port}"),
            provider: "openai",
            first_key: format!("qa-{}", common::unique_suffix()),
            replacement_key: format!("qa-{}", common::unique_suffix()),
            alias: format!("probierz/{suffix}"),
            namespace,
        })
    }

    pub(crate) fn keychain_value(&self) -> Option<String> {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-w",
                "-s",
                &self.provider_service,
                "-a",
                self.provider,
            ])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub(crate) fn delete_keychain(&self) {
        for (service, account) in [
            (&self.provider_service, self.provider),
            (&self.runtime_service, "brama-desktop"),
        ] {
            let _ = Command::new("security")
                .args(["delete-generic-password", "-s", service, "-a", account])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    pub(crate) fn route_target(&self) -> Result<Option<String>, String> {
        let raw = fs::read_to_string(&self.routes)
            .map_err(|error| format!("{}: {error}", self.routes.display()))?;
        let registry: Value = serde_json::from_str(&raw)
            .map_err(|error| format!("{}: {error}", self.routes.display()))?;
        Ok(registry
            .pointer(&format!(
                "/routes/{}",
                self.alias.replace('~', "~0").replace('/', "~1")
            ))
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    pub(crate) fn cleanup(&self) {
        self.delete_keychain();
        let _ = fs::remove_dir_all(&self.state_root);
    }
}

pub(crate) fn wait_until<F>(check: F, description: &str) -> Result<(), String>
where
    F: Fn() -> Result<bool, String>,
{
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last_error = None;
    while Instant::now() < deadline {
        match check() {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) => last_error = Some(error),
        }
        thread::sleep(Duration::from_millis(300));
    }
    Err(format!(
        "timed out waiting for {description}{}",
        last_error
            .map(|error| format!(": {error}"))
            .unwrap_or_default()
    ))
}

