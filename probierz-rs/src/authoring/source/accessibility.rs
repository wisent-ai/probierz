use serde_json::json;
use crate::authoring::*;

/// Static source/spec accessibility audit. Its JSON shape is the former JS contract.
pub fn validate_accessibility(harness: &Path, app_id: &str) -> Result<JsonValue, Failure> {
    let loaded = manifest::load(harness, app_id)?;
    let repositories = loaded
        .document
        .get("repositories")
        .and_then(YamlValue::as_sequence)
        .cloned()
        .unwrap_or_default();
    let mut modifiers = Vec::new();
    let mut prefixes = BTreeSet::new();
    let mut scanned_files = 0usize;
    for repository in &repositories {
        let root = PathBuf::from(yaml_string(repository, "root").unwrap_or_default());
        for file in files_below(&root, ".swift")? {
            scanned_files += 1;
            let content = fs::read_to_string(&file)?;
            modifiers.extend(explicit_accessibility_identifiers(&content, &file, &root));
            if content.contains(".accessibilityIdentifier") {
                prefixes.extend(swift_dynamic_prefixes(&content));
            }
        }
    }

    let basenames: BTreeSet<String> = loaded
        .document
        .get("surfaces")
        .and_then(YamlValue::as_mapping)
        .into_iter()
        .flat_map(|surfaces| surfaces.values())
        .filter_map(|surface| yaml_string(surface, "spec"))
        .filter_map(|spec| {
            Path::new(spec)
                .file_name()
                .and_then(OsStr::to_str)
                .map(str::to_string)
        })
        .collect();
    let mut spec_files = files_below(&harness.join("packages"), ".ts")?;
    spec_files.retain(|file| {
        file.file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| basenames.contains(name))
    });

    let mut references = Vec::new();
    let mut forbidden = Vec::new();
    let app_prefix = format!("{app_id}.");
    for file in &spec_files {
        let content = fs::read_to_string(file)?;
        for (value, offset) in quoted_values(&content) {
            if value.starts_with(&app_prefix) && valid_identifier(&value, true) {
                references.push(json!({ "value": value, "file": file.to_string_lossy(), "line": line_number(&content, offset) }));
            }
        }
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find("$(") {
            let start = cursor + relative;
            let mut index = start + 2;
            while content
                .as_bytes()
                .get(index)
                .is_some_and(u8::is_ascii_whitespace)
            {
                index += 1;
            }
            if let Some((selector, end)) = literal_at(&content, index) {
                let mut close = end;
                while content
                    .as_bytes()
                    .get(close)
                    .is_some_and(u8::is_ascii_whitespace)
                {
                    close += 1;
                }
                if content.as_bytes().get(close) == Some(&b')') && !selector.contains(['\'', '"']) {
                    let stable = selector.starts_with('~')
                        || selector.starts_with("[data-testid=")
                        || (selector.to_ascii_lowercase().contains("identifier") && {
                            let lower = selector.to_ascii_lowercase();
                            !(lower.contains("label=")
                                || lower.contains("name=")
                                || lower.contains("text=")
                                || lower.contains("label contains")
                                || lower.contains("name contains")
                                || lower.contains("text contains"))
                        });
                    if !stable {
                        forbidden.push(json!({ "kind": "text-selector", "selector": selector, "file": file.to_string_lossy(), "line": line_number(&content, start) }));
                    }
                }
                cursor = end;
            } else {
                cursor = index.saturating_add(1);
            }
        }
    }

    let defined: BTreeSet<String> = modifiers
        .iter()
        .filter_map(|item| item.get("value").and_then(JsonValue::as_str))
        .filter(|value| !value.contains("\\("))
        .map(str::to_string)
        .collect();
    let mut missing = Vec::new();
    let mut seen_missing = BTreeSet::new();
    for item in &references {
        let value = item["value"].as_str().unwrap_or_default();
        if !defined.contains(value)
            && !prefixes.iter().any(|prefix| value.starts_with(prefix))
            && seen_missing.insert(value.to_string())
        {
            missing.push(json!({ "kind": "missing-identifier", "identifier": value, "referencedAt": { "file": item["file"], "line": item["line"] } }));
        }
    }
    let mut locations: BTreeMap<(String, String), Vec<JsonValue>> = BTreeMap::new();
    for item in &modifiers {
        locations
            .entry((
                item["repository"].as_str().unwrap_or_default().to_string(),
                item["value"].as_str().unwrap_or_default().to_string(),
            ))
            .or_default()
            .push(json!({ "file": item["file"], "line": item["line"] }));
    }
    let mut duplicates = Vec::new();
    for ((_repository, identifier), entries) in locations {
        let files: BTreeSet<&str> = entries
            .iter()
            .filter_map(|entry| entry["file"].as_str())
            .collect();
        if files.len() > 1 {
            duplicates.push(json!({ "kind": "duplicate-identifier", "identifier": identifier, "locations": entries }));
        }
    }
    let mut errors = duplicates;
    errors.extend(missing);
    errors.extend(forbidden);
    let referenced: BTreeSet<String> = references
        .iter()
        .filter_map(|item| item["value"].as_str().map(str::to_string))
        .collect();
    let mut unused: Vec<String> = modifiers
        .iter()
        .filter_map(|item| item["value"].as_str())
        .filter(|identifier| {
            identifier.starts_with(&app_prefix) && !referenced.contains(*identifier)
        })
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    unused.sort();
    Ok(json!({
        "schemaVersion": 1,
        "appId": app_id,
        "ok": errors.is_empty(),
        "sourceFiles": scanned_files,
        "specFiles": spec_files.iter().map(|file| file.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "identifiers": { "defined": defined.len(), "explicit": modifiers.len(), "referenced": referenced.len(), "unused": unused },
        "errors": errors,
    }))
}

pub fn accessibility_command(harness: &Path, app_id: &str) -> Result<bool, Failure> {
    let result = validate_accessibility(harness, app_id)?;
    let ok = result["ok"].as_bool().unwrap_or(false);
    print_json(&result)?;
    Ok(ok)
}

