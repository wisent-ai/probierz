use crate::manifest::*;

/// Read and judge one product's manifest.
pub fn load(harness_root: &Path, app_id: &str) -> Result<Manifest, Failure> {
    let clean = app_id.trim();
    if !valid_id(clean) {
        return Err(Failure::invalid(
            "manifest.load",
            format!("invalid app ID: {app_id}"),
        ));
    }
    let file = apps_root(harness_root).join(clean).join("probierz.yaml");
    if !crate::paths::present(&file)? {
        return Err(Failure::config(
            "manifest.load",
            format!("app manifest not found: {}", file.display()),
        ));
    }
    let mut document: Value = serde_yaml::from_str(&std::fs::read_to_string(&file)?)?;
    expand_home(&mut document);
    validate(&document, &file)?;
    let Some(declared) = string_of(&document, "appId") else {
        return Err(Failure::config(
            "manifest.load",
            format!(
                "{} declares no appId, so it is not the manifest of {clean}",
                file.display()
            ),
        ));
    };
    if declared != clean {
        return Err(Failure::config(
            "manifest.load",
            format!("app manifest ID mismatch: expected {clean}, got {declared}"),
        ));
    }
    Ok(Manifest {
        app_id: declared.to_string(),
        file,
        document,
    })
}

impl Manifest {
    /// The manifest as JSON, the shape evidence, status and the gate read
    /// it in; a document that has no JSON form is refused naming its file.
    pub fn json(&self) -> Result<serde_json::Value, Failure> {
        serde_json::to_value(&self.document).map_err(|error| {
            Failure::config("manifest.json", format!("{}: {error}", self.file.display()))
        })
    }

    /// The owner validation requires every manifest to name; a document that
    /// names none is refused naming its file, never read as owner "".
    pub fn owner(&self) -> Result<&str, Failure> {
        string_of(&self.document, "owner")
            .filter(|owner| !owner.is_empty())
            .ok_or_else(|| {
                Failure::config(
                    "manifest.owner",
                    format!("{} names no owner", self.file.display()),
                )
            })
    }

    /// A path a manifest (or a file beside it) writes: absolute after `~/`
    /// expansion under the home `crate::run::home_dir` names, otherwise
    /// relative to the manifest's own directory. A `~/` left unexpanded
    /// named a directory called `~` beside the manifest.
    pub fn beside(&self, written: &str) -> PathBuf {
        let expanded = match written.strip_prefix("~/") {
            Some(rest) => crate::run::home_dir().join(rest),
            None => PathBuf::from(written),
        };
        if expanded.is_absolute() {
            return expanded;
        }
        // The manifest's file name is replaced, so the path lands in its
        // directory whether that directory is named or is the current one.
        self.file.with_file_name(expanded)
    }
}

/// The app's manifest, or `None` when the app has none yet (its manifest is
/// being authored). A manifest that exists and cannot be read or judged is
/// refused with its cause, never read as one that is not there.
pub fn load_if_declared(harness_root: &Path, app_id: &str) -> Result<Option<Manifest>, Failure> {
    let file = apps_root(harness_root)
        .join(app_id.trim())
        .join("probierz.yaml");
    if valid_id(app_id.trim()) && !crate::paths::present(&file)? {
        return Ok(None);
    }
    load(harness_root, app_id).map(Some)
}

/// A manifest names checkouts and executables under the operator's home as
/// `~/...`, so the file carries no account name and reads the same for every
/// operator. Every string that starts with `~/` is read under the home
/// `crate::run::home_dir` names; left unexpanded without HOME, it named a
/// directory called `~` under the working directory.
pub fn expand_home(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(rest) = text.strip_prefix("~/") {
                *text = crate::run::home_dir()
                    .join(rest)
                    .to_string_lossy()
                    .into_owned();
            }
        }
        Value::Sequence(items) => items.iter_mut().for_each(expand_home),
        Value::Mapping(map) => map.iter_mut().for_each(|(_, item)| expand_home(item)),
        Value::Tagged(tagged) => expand_home(&mut tagged.value),
        _ => {}
    }
}

/// The root of the manifest's first repository, the product's own source.
pub fn primary_root(manifest: &Manifest) -> Option<PathBuf> {
    manifest
        .document
        .get("repositories")?
        .as_sequence()?
        .first()?
        .get("root")?
        .as_str()
        .filter(|root| !root.trim().is_empty())
        .map(PathBuf::from)
}

/// Every product that declares a manifest, in the order an operator reads.
pub fn list(harness_root: &Path) -> Result<Vec<AppSummary>, Failure> {
    let root = apps_root(harness_root);
    if !crate::paths::present(&root)? {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if !crate::paths::present(&entry.path().join("probierz.yaml"))? {
            continue;
        }
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    let mut summaries = Vec::with_capacity(names.len());
    for name in names {
        let manifest = load(harness_root, &name)?;
        summaries.push(AppSummary {
            app_id: manifest.app_id.clone(),
            owner: manifest.owner()?.to_string(),
            file: manifest.file.to_string_lossy().into_owned(),
            targets: sorted_keys(&manifest.document, "surfaces"),
            journeys: sorted_keys(&manifest.document, "journeys"),
        });
    }
    Ok(summaries)
}

pub(crate) fn sorted_keys(document: &Value, key: &str) -> Vec<String> {
    // A manifest that declares no mapping under `key` names no keys there.
    let Some(mapping) = map_of(document, key) else {
        return Vec::new();
    };
    let mut keys = key_names(mapping);
    keys.sort();
    keys
}

/// The journeys a surface runs, after the first override whose conditions all
/// match. An unmatched override, or one that states no conditions, never
/// contributes; a surface or override that lists no journeys runs none.
pub fn surface_journeys(surface: &Value, environment: &BTreeMap<String, String>) -> Vec<String> {
    if let Some(overrides) = sequence_of(surface, "journeyOverrides") {
        for override_entry in overrides {
            let Some(when) = map_of(override_entry, "when") else {
                continue;
            };
            let matches = when.iter().all(|(key, value)| {
                let Some(name) = key.as_str() else {
                    return false;
                };
                let wanted = match value {
                    Value::String(text) => text.clone(),
                    Value::Number(number) => number.to_string(),
                    Value::Bool(flag) => flag.to_string(),
                    _ => return false,
                };
                // An unset variable matches no value, not the empty one.
                environment.get(name).map(String::as_str) == Some(wanted.as_str())
            });
            if matches {
                return listed_journeys(override_entry);
            }
        }
    }
    listed_journeys(surface)
}

/// The journey names `entry` lists under `journeys`.
fn listed_journeys(entry: &Value) -> Vec<String> {
    match sequence_of(entry, "journeys") {
        Some(list) => list
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        None => Vec::new(),
    }
}
