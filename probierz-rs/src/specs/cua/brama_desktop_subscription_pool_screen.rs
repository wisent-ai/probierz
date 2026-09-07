use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{json, Value};

use crate::{cua, specs};

use super::common;

const EXPECTED: [(&str, &str, &str, Option<&str>); 4] = [
    ("sub-anthropic-7f21", "anthropic", "Live", None),
    (
        "sub-google-93bd",
        "google",
        "Expired",
        Some("the pooled grant expired before the last dispatch"),
    ),
    ("sub-mistral-4a88", "mistral", "Unknown", None),
    (
        "sub-openai-1c04",
        "openai",
        "Burnt",
        Some("the provider refused the stored grant: seat revoked"),
    ),
];

struct Fixture {
    scratch: PathBuf,
    wrapper: PathBuf,
    ledger: PathBuf,
    router: PathBuf,
    invocations: PathBuf,
    runtime: PathBuf,
}

impl Fixture {
    fn new(executable: &std::path::Path) -> Result<Self, String> {
        let home = std::env::var("HOME")
            .map_err(|_| "HOME is required to build the Brama subscription fixture".to_string())?;
        let scratch =
            PathBuf::from(home).join("Library/Caches/probierz-vg-journeys/brama-subscription-pool");
        Ok(Self {
            wrapper: scratch.join("brama"),
            ledger: scratch.join("subscription-usage.json"),
            router: scratch.join("entitlements-router"),
            invocations: scratch.join("invocations.log"),
            runtime: executable
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("brama-runtime"),
            scratch,
        })
    }

    fn build(&self) -> Result<(), String> {
        if !self.runtime.is_file() {
            return Err(format!(
                "the bundled brama runtime is required at {}",
                self.runtime.display()
            ));
        }
        let _ = fs::remove_dir_all(&self.scratch);
        fs::create_dir_all(&self.scratch)
            .map_err(|error| format!("{}: {error}", self.scratch.display()))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let day = 24_i64 * 60 * 60 * 1000;
        let ledger = json!({
            "subscriptions": {
                "sub-anthropic-7f21": {
                    "provider": "anthropic",
                    "credential": {"state": "active", "recorded_at_ms": now - 3_600_000, "expires_at_ms": now + 120 * day},
                    "credential_value": "sk-probierz-NEVERRENDER-anthropic"
                },
                "sub-openai-1c04": {
                    "provider": "openai",
                    "credential": {"state": "needs_reauthorization", "cause": "the provider refused the stored grant: seat revoked", "recorded_at_ms": now - 3_600_000},
                    "credential_value": "sk-probierz-NEVERRENDER-openai"
                },
                "sub-google-93bd": {
                    "provider": "google",
                    "credential": {"state": "active", "recorded_at_ms": now - 3_600_000, "expires_at_ms": now - 9 * day},
                    "probe": {"attempted_at_ms": now - 3_600_000, "ok": false, "detail": "the pooled grant expired before the last dispatch"},
                    "credential_value": "sk-probierz-NEVERRENDER-google"
                },
                "sub-mistral-4a88": {"provider": "mistral", "credential_value": "sk-probierz-NEVERRENDER-mistral"}
            }
        });
        fs::write(
            &self.ledger,
            format!("{}\n", serde_json::to_string_pretty(&ledger).unwrap()),
        )
        .map_err(|error| format!("{}: {error}", self.ledger.display()))?;
        fs::write(&self.router, "#!/bin/sh\nexit 1\n").map_err(|error| error.to_string())?;
        fs::set_permissions(&self.router, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nif [ \"$1\" = \"subscription\" ] && [ \"$2\" = \"refresh\" ]; then\n  printf 'error: this probierz journey never refreshes a pooled subscription\\n' >&2\n  exit 3\nfi\nBRAMA_SUBSCRIPTION_USAGE_FILE={} \\\nENTITLEMENTS_ROUTER_BIN={} \\\nexec {} \"$@\"\n",
            shell_quote(&self.invocations), shell_quote(&self.ledger), shell_quote(&self.router), shell_quote(&self.runtime)
        );
        fs::write(&self.wrapper, script).map_err(|error| error.to_string())?;
        fs::set_permissions(&self.wrapper, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
        fs::write(&self.invocations, "").map_err(|error| error.to_string())?;
        Ok(())
    }

    fn invocations(&self) -> Result<Vec<String>, String> {
        Ok(fs::read_to_string(&self.invocations)
            .map_err(|error| format!("{}: {error}", self.invocations.display()))?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }
}

fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn assert_no_secret(tree: &str, where_: &str) -> Result<(), String> {
    for (name, pattern) in [
        ("the ledger decoy", r"NEVERRENDER"),
        ("an API-key-shaped string", r"\bsk-[A-Za-z0-9_-]{6,}"),
        ("a bearer token", r"Bearer\s+[A-Za-z0-9._-]{8,}"),
        ("a JWT", r"\bey[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"),
    ] {
        if Regex::new(pattern).unwrap().is_match(tree) {
            return Err(format!(
                "{where_} must render no credential material ({name})"
            ));
        }
    }
    Ok(())
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let executable =
        common::executable(context, "path to the Brama native application executable")?;
    let fixture = Fixture::new(&executable)?;
    fixture.build()?;
    let driver = common::driver(context)?;
    let environment = BTreeMap::from([
        (
            "BRAMA_BIN".to_string(),
            fixture.wrapper.to_string_lossy().into_owned(),
        ),
        (
            "BRAMA_SUBSCRIPTION_USAGE_FILE".to_string(),
            fixture.ledger.to_string_lossy().into_owned(),
        ),
        (
            "ENTITLEMENTS_ROUTER_BIN".to_string(),
            fixture.router.to_string_lossy().into_owned(),
        ),
    ]);
    let app = driver.launch_process(&executable, &environment, &[])?;
    let result = (|| {
        driver.bring_to_front(app.pid, app.window_id)?;
        std::thread::sleep(Duration::from_millis(1500));
        let (shell_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Subscription Pool",
            Duration::from_secs(60),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            shell_window,
            "the Subscription Pool destination",
            |element| {
                cua::element_label(element) == "Subscription Pool" && common::is_button(element)
            },
            |tree| tree.contains("AXStaticText = \"LAST REDEEM ERROR\""),
            Duration::from_secs(20),
        )?;
        let (window_id, loaded) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "AXStaticText = \"LAST REDEEM ERROR\"",
            Duration::from_secs(60),
        )?;
        common::dump_tree(context, "pool-loaded", &loaded.tree)?;
        common::capture(context, &driver, app.pid, window_id, "pool-loaded")?;
        let texts: HashSet<String> = common::static_texts(&loaded.tree).into_iter().collect();
        for (text, message) in [
            ("Subscription Pool", "the screen should render its title"),
            (
                "brama CLI",
                "the screen should scope itself to the CLI it read",
            ),
        ] {
            if !texts.contains(text) {
                return Err(message.to_string());
            }
        }
        for (needle, message) in [
            (
                "AXStaticText = \"The subscription pool could not be read\"",
                "the pool read should not have failed",
            ),
            (
                "AXStaticText = \"Reading the subscription pool\"",
                "the screen should not still be reading once the pool is on it",
            ),
            (
                "AXStaticText = \"Not read yet\"",
                "the screen should report when it read the pool",
            ),
            (
                "AXStaticText = \"Reading…\"",
                "no read should still be in flight",
            ),
            (
                "AXStaticText = \"The pool holds no subscription\"",
                "the ledger pool is not empty",
            ),
            (
                "AXStaticText = \"No subscription in the pool is live\"",
                "one ledger subscription is live",
            ),
        ] {
            if loaded.tree.contains(needle) {
                return Err(message.to_string());
            }
        }
        if !texts.iter().any(|text| text.starts_with("read ")) {
            return Err("the screen should show the read's freshness".to_string());
        }
        for column in [
            "PROVIDER",
            "SUBSCRIPTION",
            "STATE",
            "EXPIRES",
            "LAST REDEEM ERROR",
        ] {
            if !texts.contains(column) {
                return Err(format!("the table should render the {column} column"));
            }
        }
        for (id, provider, state, error) in EXPECTED {
            let row = format!("({provider}, {id}, {state}, ");
            if !loaded.tree.contains(&row) {
                return Err(format!(
                    "the table should render {provider} as {state} with its identity"
                ));
            }
            if error.is_some_and(|error| !loaded.tree.contains(error)) {
                return Err(format!(
                    "the table should render the provider's own refusal for {provider}"
                ));
            }
        }
        if !loaded.tree.contains("No expiry recorded") {
            return Err(
                "a pooled subscription whose credential states no expiry should say so".to_string(),
            );
        }
        for (signal, count) in [("Live", 1), ("Burnt", 1), ("Expired", 1), ("Unknown", 1)] {
            if !texts.contains(&format!("{signal}: {count}")) {
                return Err(format!(
                    "the pool should count {count} {} subscription",
                    signal.to_lowercase()
                ));
            }
        }
        let position = |provider: &str| {
            loaded
                .tree
                .find(&format!("({provider}, "))
                .unwrap_or(usize::MAX)
        };
        for unusable in ["google", "mistral", "openai"] {
            if position(unusable) >= position("anthropic") {
                return Err(format!(
                    "the unusable {unusable} row should sort above the live one"
                ));
            }
        }
        assert_no_secret(&loaded.tree, "the loaded pool")?;

        common::activate(
            context,
            &driver,
            app.pid,
            window_id,
            "the openai row",
            |element| {
                cua::element_label(element).starts_with("openai, ") && common::is_button(element)
            },
            |tree| tree.contains("POOLED SUBSCRIPTION"),
            Duration::from_secs(15),
        )?;
        let (inspector_window, inspector) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "POOLED SUBSCRIPTION",
            Duration::from_secs(30),
        )?;
        common::dump_tree(context, "pool-inspector", &inspector.tree)?;
        for label in [
            "IDENTITY",
            "PROVIDER",
            "SUBSCRIPTION ID",
            "STATE",
            "EXPIRY",
            "REFRESH",
        ] {
            if !inspector.tree.contains(label) {
                return Err(format!("the inspector should render its {label} field"));
            }
        }
        for (needle, message) in [
            (
                "sub-openai-1c04",
                "the inspector should name the pooled subscription it is describing",
            ),
            (
                "Never available here",
                "the inspector should state what this screen never shows",
            ),
            (
                "Reading the credential value behind a pooled subscription",
                "the inspector should name the credential value as unavailable",
            ),
        ] {
            if !inspector.tree.contains(needle) {
                return Err(message.to_string());
            }
        }
        assert_no_secret(&inspector.tree, "the inspector")?;
        let before = fixture.invocations()?;
        common::activate(
            context,
            &driver,
            app.pid,
            inspector_window,
            "the Refresh openai action",
            |element| {
                cua::element_label(element).starts_with("Refresh openai")
                    && common::is_button(element)
            },
            |tree| tree.contains("Refresh the openai subscription pool?"),
            Duration::from_secs(20),
        )?;
        let (dialog_window, dialog) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "AXStaticText = \"Refresh the openai subscription pool?\"",
            Duration::from_secs(30),
        )?;
        common::dump_tree(context, "refresh-empty-reason", &dialog.tree)?;
        if !dialog
            .tree
            .contains("AXStaticText = \"A reason is required. The command refuses without one.\"")
        {
            return Err("an empty reason should be refused in the dialog's own words".to_string());
        }
        if !dialog
            .tree
            .contains("brama subscription refresh openai --reason '' --json")
        {
            return Err(
                "the previewed command should show the empty reason it would carry".to_string(),
            );
        }
        common::capture(context, &driver, app.pid, dialog_window, "refresh-refused")?;
        let state = driver.snapshot(app.pid, dialog_window)?;
        let confirms: Vec<&Value> = state
            .elements
            .iter()
            .filter(|element| {
                cua::element_label(element) == "Refresh it" && common::is_button(element)
            })
            .collect();
        if !dialog.tree.contains("AXButton (Refresh it)") {
            return Err(
                "the dialog should render the confirm button it is refusing to run".to_string(),
            );
        }
        let mut press_refusal = confirms.is_empty().then(|| {
            "the Refresh it button exposes no press action while the reason is empty".to_string()
        });
        if let Some(confirm) = confirms.first() {
            if let Err(error) = driver.click_element(app.pid, dialog_window, &state, confirm) {
                press_refusal = Some(error);
            }
        }
        std::thread::sleep(Duration::from_millis(2500));
        let after_press = driver.snapshot(app.pid, dialog_window)?.tree;
        common::dump_tree(context, "refresh-after-press", &after_press)?;
        if !after_press
            .contains("AXStaticText = \"A reason is required. The command refuses without one.\"")
        {
            return Err(format!(
                "the dialog should still refuse after the action was invoked{}",
                press_refusal
                    .map(|message| format!(" (press refused: {message})"))
                    .unwrap_or_default()
            ));
        }
        let after = fixture.invocations()?;
        if after != before {
            return Err("a refused refresh must not invoke the Brama CLI".to_string());
        }
        if after
            .iter()
            .any(|line| line.starts_with("subscription refresh"))
        {
            return Err(format!(
                "the CLI must never be asked to refresh a pool here: {}",
                after.join(" | ")
            ));
        }
        if !after
            .iter()
            .any(|line| line.starts_with("subscriptions list"))
        {
            return Err("the pool on screen must come from a real CLI read".to_string());
        }
        let shell_after = driver.snapshot(app.pid, window_id)?.tree;
        if shell_after.contains("AXStaticText = \"Refreshing openai credentials.") {
            return Err("no refresh may be in flight".to_string());
        }
        Ok(())
    })();
    driver.quit_app(app.pid);
    result
}
