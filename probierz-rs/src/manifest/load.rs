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
    if !file.exists() {
        return Err(Failure::config(
            "manifest.load",
            format!("app manifest not found: {}", file.display()),
        ));
    }
    let mut document: Value = serde_yaml::from_str(&std::fs::read_to_string(&file)?)?;
    expand_home(&mut document);
    validate(&document, &file)?;
    let declared = string_of(&document, "appId").unwrap_or_default();
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

/// A manifest names checkouts and executables under the operator's home as
/// `~/...`, so the file carries no account name and reads the same for every
/// operator. Every string that starts with `~/` is read under `$HOME`.
pub fn expand_home(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let (Some(rest), Some(home)) = (text.strip_prefix("~/"), std::env::var_os("HOME")) {
                *text = Path::new(&home).join(rest).to_string_lossy().into_owned();
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
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if !entry.path().join("probierz.yaml").exists() {
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
            owner: string_of(&manifest.document, "owner")
                .unwrap_or_default()
                .to_string(),
            file: manifest.file.to_string_lossy().into_owned(),
            targets: sorted_keys(&manifest.document, "surfaces"),
            journeys: sorted_keys(&manifest.document, "journeys"),
        });
    }
    Ok(summaries)
}

pub(crate) fn sorted_keys(document: &Value, key: &str) -> Vec<String> {
    let mut keys: Vec<String> = map_of(document, key).map(key_names).unwrap_or_default();
    keys.sort();
    keys
}

/// The journeys a surface runs, after the first override whose conditions all
/// match. An unmatched override never contributes.
pub fn surface_journeys(surface: &Value, environment: &BTreeMap<String, String>) -> Vec<String> {
    for override_entry in sequence_of(surface, "journeyOverrides").unwrap_or(&Vec::new()) {
        let when = map_of(override_entry, "when");
        let matches = when
            .map(|map| {
                map.iter().all(|(key, value)| {
                    let name = key.as_str().unwrap_or_default();
                    let wanted = match value {
                        Value::String(text) => text.clone(),
                        Value::Number(number) => number.to_string(),
                        Value::Bool(flag) => flag.to_string(),
                        _ => return false,
                    };
                    environment.get(name).map(String::as_str).unwrap_or("") == wanted
                })
            })
            .unwrap_or(false);
        if matches {
            return sequence_of(override_entry, "journeys")
                .map(|list| {
                    list.iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
        }
    }
    sequence_of(surface, "journeys")
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
