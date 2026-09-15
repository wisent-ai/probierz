//! The real vault and route table this journey reads.
//!
//! Everything here goes through the Skarbiec release CLI, so the table
//! the application reads is one the product itself wrote. The fixture
//! also records the table's exact state before the journey, so a
//! refused action can be shown to have changed nothing.

use super::*;

/// Where the Skarbiec CLI is expected when the run does not name it.
const DEFAULT_CLI: &str =
    "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec";

/// How much of a non-JSON answer is quoted back.
const ANSWER_EXCERPT: usize = 300;

/// The reason recorded with every route the fixture adds.
const FIXTURE_REASON: &str = "probierz capabilities-screen fixture";

pub(crate) struct Fixture {
    pub(crate) scratch: PathBuf,
    pub(crate) vault: PathBuf,
    pub(crate) audit: PathBuf,
    pub(crate) routes: PathBuf,
    pub(crate) routes_audit: PathBuf,
    pub(crate) cli: PathBuf,
    pub(crate) path: String,
}

impl Fixture {
    pub(crate) fn new(context: &specs::Context) -> Result<Self, String> {
        let home = std::env::var("HOME").map_err(|_| {
            "HOME is required to build the Skarbiec capabilities fixture".to_string()
        })?;
        let scratch =
            PathBuf::from(home).join("Library/Caches/probierz-vg-journeys/skarbiec-capabilities");
        let cli = common::optional_path(context, "PROBIERZ_SKARBIEC_CLI", DEFAULT_CLI);
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

    pub(crate) fn command(&self, arguments: &[&str]) -> Result<String, String> {
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

    pub(crate) fn json(&self, arguments: &[&str]) -> Result<Value, String> {
        let raw = self.command(arguments)?;
        serde_json::from_str(&raw).map_err(|_| {
            format!(
                "skarbiec {} did not answer with JSON: {}",
                arguments.join(" "),
                common::tail(raw.trim(), ANSWER_EXCERPT)
            )
        })
    }

    /// Build the vault, the item, and the three routes — then read the
    /// table back through the CLI and require it to answer exactly the
    /// resolutions this journey expects to see on screen.
    pub(crate) fn build(&self) -> Result<(), String> {
        if !self.cli.is_file() {
            return Err(format!(
                "the skarbiec release CLI is required at {}",
                self.cli.display()
            ));
        }
        fs::create_dir_all(&self.scratch)
            .map_err(|error| format!("{}: {error}", self.scratch.display()))?;
        self.clear_route_tables()?;

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
                FIXTURE_REASON,
                "--json",
            ])?;
        }
        self.assert_table()
    }

    /// Remove any route table an earlier run left, so the journey
    /// starts from the three routes it declares and nothing else.
    fn clear_route_tables(&self) -> Result<(), String> {
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
        Ok(())
    }

    /// The CLI's own view of the table the application will read.
    fn assert_table(&self) -> Result<(), String> {
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

    /// A digest of every route table file, so a refused action can be
    /// shown to have left all of them byte-identical.
    pub(crate) fn fingerprint(&self) -> Result<String, String> {
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

    /// The table backups Skarbiec publishes when it rewrites the table.
    pub(crate) fn backups(&self) -> Result<HashSet<String>, String> {
        Ok(fs::read_dir(&self.scratch)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("json.before-"))
            .collect())
    }

    /// The environment the application is launched with: this
    /// fixture's CLI, vault, audit log and PATH.
    pub(crate) fn environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "SKARBIEC_CLI".to_string(),
                self.cli.to_string_lossy().into_owned(),
            ),
            (
                "SKARBIEC_VAULT_FILE".to_string(),
                self.vault.to_string_lossy().into_owned(),
            ),
            (
                "SKARBIEC_AUDIT_FILE".to_string(),
                self.audit.to_string_lossy().into_owned(),
            ),
            ("PATH".to_string(), self.path.clone()),
        ])
    }
}

/// The table's exact state before the journey touched anything.
pub(crate) struct TableBefore {
    pub(crate) fingerprint: String,
    pub(crate) routes: Vec<u8>,
    pub(crate) audit: Vec<u8>,
    pub(crate) backups: HashSet<String>,
}

impl TableBefore {
    pub(crate) fn read(fixture: &Fixture) -> Result<Self, String> {
        Ok(Self {
            fingerprint: fixture.fingerprint()?,
            routes: fs::read(&fixture.routes)
                .map_err(|error| format!("{}: {error}", fixture.routes.display()))?,
            audit: fs::read(&fixture.routes_audit).unwrap_or_default(),
            backups: fixture.backups()?,
        })
    }
}

pub(crate) fn assert_static(
    texts: &HashSet<String>,
    text: &str,
    message: &str,
) -> Result<(), String> {
    if texts.contains(text) {
        Ok(())
    } else {
        Err(message.to_string())
    }
}
