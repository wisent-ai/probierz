use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{cua, specs};

use super::common;

const FIXTURE_ROUTES: [(&str, &str, &str, &str); 3] = [
    (
        "https://login.example.com",
        "example-login",
        "password",
        "Resolves",
    ),
    (
        "https://sso.example.com",
        "example-login",
        "totp",
        "Field missing",
    ),
    (
        "https://absent.example.com",
        "missing-login",
        "password",
        "Item unreadable",
    ),
];

struct Fixture {
    scratch: PathBuf,
    vault: PathBuf,
    audit: PathBuf,
    routes: PathBuf,
    routes_audit: PathBuf,
    cli: PathBuf,
    path: String,
}

impl Fixture {
    fn new(context: &specs::Context) -> Result<Self, String> {
        let home = std::env::var("HOME").map_err(|_| {
            "HOME is required to build the Skarbiec capabilities fixture".to_string()
        })?;
        let scratch =
            PathBuf::from(home).join("Library/Caches/probierz-vg-journeys/skarbiec-capabilities");
        let cli = common::optional_path(
            context,
            "PROBIERZ_SKARBIEC_CLI",
            "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec",
        );
        let path = format!(
            "/opt/homebrew/bin:{}",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string())
        );
        Ok(Self {
            vault: scratch.join("skarbiec.vault"),
            audit: scratch.join("audit.log"),
            routes: scratch.join("capability-routes.json"),
            routes_audit: scratch.join("capability-routes.audit.jsonl"),
            scratch,
            cli,
            path,
        })
    }

    fn command(&self, arguments: &[&str]) -> Result<String, String> {
        let output = Command::new(&self.cli)
            .args(arguments)
            .env("PATH", &self.path)
            .env("SKARBIEC_VAULT_FILE", &self.vault)
            .env("SKARBIEC_AUDIT_FILE", &self.audit)
            .output()
            .map_err(|error| format!("{}: {error}", self.cli.display()))?;
        if !output.status.success() {
            return Err(format!(
                "skarbiec {} failed: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn json(&self, arguments: &[&str]) -> Result<Value, String> {
        let raw = self.command(arguments)?;
        serde_json::from_str(&raw).map_err(|_| {
            format!(
                "skarbiec {} did not answer with JSON: {}",
                arguments.join(" "),
                common::tail(raw.trim(), 300)
            )
        })
    }

    fn build(&self) -> Result<(), String> {
        if !self.cli.is_file() {
            return Err(format!(
                "the skarbiec release CLI is required at {}",
                self.cli.display()
            ));
        }
        fs::create_dir_all(&self.scratch)
            .map_err(|error| format!("{}: {error}", self.scratch.display()))?;
        for entry in fs::read_dir(&self.scratch).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("capability-routes")
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        if !self.vault.exists() {
            self.json(&["init", "probierz-capabilities-fixture", "--json"])?;
        }
        self.json(&[
            "set",
            "example-login",
            "username=agent@example.com",
            "password=synthetic-fixture-value",
            "--json",
        ])?;
        for (resource, item, field, _) in FIXTURE_ROUTES {
            self.json(&[
                "routes",
                "add",
                "--resource",
                resource,
                "--item",
                item,
                "--field",
                field,
                "--reason",
                "probierz capabilities-screen fixture",
                "--json",
            ])?;
        }
        let listed = self.json(&["routes", "list", "--json"])?;
        let rows = listed
            .get("routes")
            .and_then(Value::as_array)
            .ok_or_else(|| "fixture routes list did not contain routes".to_string())?;
        let by_resource: HashMap<&str, &Value> = rows
            .iter()
            .filter_map(|row| Some((row.get("resource")?.as_str()?, row)))
            .collect();
        for (resource, item, field, resolution) in FIXTURE_ROUTES {
            let row = by_resource.get(resource).ok_or_else(|| {
                format!("fixture route {resource} should be in the table the app will read")
            })?;
            if row.get("item").and_then(Value::as_str) != Some(item)
                || row.get("field").and_then(Value::as_str) != Some(field)
                || row.get("item_present").and_then(Value::as_bool)
                    != Some(resolution != "Item unreadable")
                || row.get("field_present").and_then(Value::as_bool)
                    != Some(resolution == "Resolves")
            {
                return Err(format!(
                    "fixture route {resource} does not answer with the expected resolution: {row}"
                ));
            }
        }
        if rows.len() != FIXTURE_ROUTES.len() {
            return Err(format!(
                "the fixture table should contain exactly {} routes, found {}",
                FIXTURE_ROUTES.len(),
                rows.len()
            ));
        }
        Ok(())
    }

    fn fingerprint(&self) -> Result<String, String> {
        let mut entries: Vec<PathBuf> = fs::read_dir(&self.scratch)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("capability-routes")
            })
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        let mut rows = Vec::new();
        for path in entries {
            let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let digest = format!("{:x}", Sha256::digest(bytes));
            rows.push(format!(
                "{}:{digest}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        Ok(rows.join("\n"))
    }

    fn backups(&self) -> Result<HashSet<String>, String> {
        Ok(fs::read_dir(&self.scratch)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("json.before-"))
            .collect())
    }
}

fn assert_static(texts: &HashSet<String>, text: &str, message: &str) -> Result<(), String> {
    if texts.contains(text) {
        Ok(())
    } else {
        Err(message.to_string())
    }
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let executable = common::executable(
        context,
        "path to the Skarbiec native application executable",
    )?;
    let fixture = Fixture::new(context)?;
    fixture.build()?;
    let fingerprint_before = fixture.fingerprint()?;
    let routes_before = fs::read(&fixture.routes)
        .map_err(|error| format!("{}: {error}", fixture.routes.display()))?;
    let audit_before = fs::read(&fixture.routes_audit).unwrap_or_default();
    let backups_before = fixture.backups()?;

    let driver = common::driver(context)?;
    let environment = BTreeMap::from([
        (
            "SKARBIEC_CLI".to_string(),
            fixture.cli.to_string_lossy().into_owned(),
        ),
        (
            "SKARBIEC_VAULT_FILE".to_string(),
            fixture.vault.to_string_lossy().into_owned(),
        ),
        (
            "SKARBIEC_AUDIT_FILE".to_string(),
            fixture.audit.to_string_lossy().into_owned(),
        ),
        ("PATH".to_string(), fixture.path.clone()),
    ]);
    let app = driver.launch_process(&executable, &environment, &[])?;
    let result = (|| {
        driver.bring_to_front(app.pid, app.window_id)?;
        std::thread::sleep(Duration::from_millis(1500));
        let (shell_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "AXButton (Capabilities)",
            Duration::from_secs(60),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            shell_window,
            "the Capabilities destination",
            |element| cua::element_label(element) == "Capabilities" && common::is_button(element),
            |tree| tree.contains("AXButton (Read routes)"),
            Duration::from_secs(15),
        )?;
        let (window_id, loaded) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "AXStaticText = \"https://login.example.com\"",
            Duration::from_secs(90),
        )?;
        common::dump_tree(context, "capabilities-loaded", &loaded.tree)?;
        common::capture(context, &driver, app.pid, window_id, "capabilities-loaded")?;
        let texts: HashSet<String> = common::static_texts(&loaded.tree).into_iter().collect();
        assert_static(&texts, "Capabilities", "the screen should render its title")?;
        if !loaded.tree.contains("(every consumer)") {
            return Err("the screen should say which consumer it read routes for".to_string());
        }
        if !loaded.tree.contains("AXButton (Read routes)") {
            return Err("the screen should render its read control".to_string());
        }
        assert_static(
            &texts,
            "3 routes, 2 unresolved",
            "the context bar should count the table and the routes that do not resolve",
        )?;
        for (needle, message) in [
            (
                "AXStaticText = \"Reading capability routes\"",
                "the screen should not still be loading once the routes are on it",
            ),
            (
                "AXStaticText = \"No capability routes\"",
                "the fixture table is not empty",
            ),
            (
                "AXStaticText = \"not read\"",
                "the screen should have read the table",
            ),
            (
                "Verifying routes against the vault",
                "no verification should be in flight",
            ),
        ] {
            if loaded.tree.contains(needle) {
                return Err(message.to_string());
            }
        }
        for column in ["RESOURCE", "ITEM", "FIELD", "RESOLVES"] {
            if !loaded.tree.contains(&format!("AXButton \"{column}\"")) {
                return Err(format!("the table should render the {column} column"));
            }
        }
        for (resource, item, field, resolution) in FIXTURE_ROUTES {
            assert_static(
                &texts,
                resource,
                &format!("the table should render the route for {resource}"),
            )?;
            assert_static(
                &texts,
                item,
                &format!("the table should render the item {item} for {resource}"),
            )?;
            assert_static(
                &texts,
                field,
                &format!("the table should render the field {field} for {resource}"),
            )?;
            if !loaded.tree.contains(&format!("({resolution})")) {
                return Err(format!(
                    "the table should resolve {resource} as {resolution}"
                ));
            }
        }
        if !loaded
            .tree
            .contains("One route names a field its item does not carry")
        {
            return Err(
                "a route whose item lacks the named field should be called out".to_string(),
            );
        }
        if !loaded
            .tree
            .contains("One route names an item this host cannot read")
        {
            return Err(
                "a route whose item this host cannot read should be called out separately"
                    .to_string(),
            );
        }
        let position = |resource: &str| {
            loaded
                .tree
                .find(&format!("AXStaticText = \"{resource}\""))
                .unwrap_or(usize::MAX)
        };
        if position("https://sso.example.com") >= position("https://absent.example.com") {
            return Err(
                "the field-missing route should sort above the unreadable-item route".to_string(),
            );
        }
        if position("https://absent.example.com") >= position("https://login.example.com") {
            return Err("unresolved routes should sort above the route that resolves".to_string());
        }

        common::activate(
            context,
            &driver,
            app.pid,
            window_id,
            "the Add route action",
            |element| cua::element_label(element) == "Add route" && common::is_button(element),
            |tree| tree.contains("AXStaticText = \"Add a route\""),
            Duration::from_secs(15),
        )?;
        common::type_field(
            context,
            &driver,
            app.pid,
            window_id,
            "Resource",
            "https://probierz.example.com",
            true,
        )?;
        common::type_field(
            context,
            &driver,
            app.pid,
            window_id,
            "Item",
            "example-login",
            true,
        )?;
        common::type_field(
            context, &driver, app.pid, window_id, "Field", "password", true,
        )?;
        let filled = driver.snapshot(app.pid, window_id)?;
        common::dump_tree(context, "add-route-empty-reason", &filled.tree)?;
        if !filled
            .tree
            .contains("AXStaticText = \"A reason is required and is recorded with the change.\"")
        {
            return Err("an empty reason should be refused in the drawer's own words".to_string());
        }
        if !filled.tree.contains("--reason '' --json") {
            return Err(
                "the previewed command should show the empty reason it would carry".to_string(),
            );
        }
        common::capture(context, &driver, app.pid, window_id, "add-route-refused")?;
        let state = driver.snapshot(app.pid, window_id)?;
        let named: Vec<&Value> = state
            .elements
            .iter()
            .filter(|element| {
                cua::element_label(element) == "Add route" && common::is_button(element)
            })
            .collect();
        let rendered = filled.tree.matches("AXButton (Add route)").count();
        if rendered < 2 {
            return Err(
                "the drawer should render its own Add route button beside the action bar's"
                    .to_string(),
            );
        }
        let mut press_refusal = (named.len() < rendered).then(|| {
            "the drawer's Add route button exposes no press action while the reason is empty"
                .to_string()
        });
        if press_refusal.is_none() {
            if let Some(submit) = named.into_iter().max_by(|left, right| {
                let y = |element: &Value| {
                    cua::element_frame(element)
                        .map(|frame| frame.y)
                        .unwrap_or(0.0)
                };
                y(left).total_cmp(&y(right))
            }) {
                if let Err(error) = driver.click_element(app.pid, window_id, &state, submit) {
                    press_refusal = Some(error);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(2500));
        let after = driver.snapshot(app.pid, window_id)?.tree;
        common::dump_tree(context, "add-route-after-press", &after)?;
        if !after
            .contains("AXStaticText = \"A reason is required and is recorded with the change.\"")
        {
            return Err(format!(
                "the drawer should still refuse after the action was invoked{}",
                press_refusal
                    .map(|message| format!(" (press refused: {message})"))
                    .unwrap_or_default()
            ));
        }
        if after.contains("Route added") {
            return Err("no route may be written without a reason".to_string());
        }
        if fixture.fingerprint()? != fingerprint_before {
            return Err("the capability routes table must be untouched".to_string());
        }
        if fs::read(&fixture.routes).unwrap_or_default() != routes_before {
            return Err("the routes table content must be unchanged".to_string());
        }
        if fs::read(&fixture.routes_audit).unwrap_or_default() != audit_before {
            return Err("no audit line may be written for a refused route".to_string());
        }
        let new_backups: Vec<String> = fixture
            .backups()?
            .difference(&backups_before)
            .cloned()
            .collect();
        if !new_backups.is_empty() {
            return Err("a refused add must not publish a new table backup".to_string());
        }
        Ok(())
    })();
    driver.quit_app(app.pid);
    result
}
