use crate::authoring::*;
use serde_json::json;

/// The schema of the accessibility source report.
const REPORT_SCHEMA_VERSION: u64 = 1;

/// Audits the accessibility identifiers of `app_id` from source alone: every
/// literal `.accessibilityIdentifier("…")` in the Swift files below each
/// declared repository root, against every `<app>.…` identifier and `$(…)`
/// selector the app's surface specs under `packages/` quote. The answer lists
/// duplicate identifiers, referenced identifiers no source defines, and
/// selectors that match visible text rather than an identifier. A manifest
/// whose repositories, surfaces or specs are not of their declared shape is
/// refused naming the field, never read as nothing to audit.
pub fn validate_accessibility(harness: &Path, app_id: &str) -> Result<JsonValue, Failure> {
    let loaded = manifest::load(harness, app_id)?;
    let refused =
        |detail: String| Failure::config("authoring.accessibility", format!("app {app_id} {detail}"));
    let repositories = match loaded.document.get("repositories") {
        Some(YamlValue::Sequence(list)) => list,
        Some(other) => return Err(refused(format!("declares repositories as {other:?}, not a list"))),
        None => return Err(refused("declares no repositories, so there is no source to audit".to_string())),
    };
    let mut modifiers = Vec::new();
    let mut prefixes = BTreeSet::new();
    let mut scanned_files = 0usize;
    for repository in repositories {
        let root = PathBuf::from(
            crate::gate::yaml_string(crate::gate::yaml_get(repository, "root"))
                .ok_or_else(|| refused("declares a repository without a text root".to_string()))?,
        );
        for file in files_below(&root, ".swift")? {
            scanned_files += 1;
            let content = fs::read_to_string(&file)?;
            modifiers.extend(explicit_accessibility_identifiers(&content, &file, &root));
            if content.contains(".accessibilityIdentifier") {
                prefixes.extend(swift_dynamic_prefixes(&content));
            }
        }
    }

    let surfaces = match loaded.document.get("surfaces") {
        None => None,
        Some(YamlValue::Mapping(surfaces)) => Some(surfaces),
        Some(other) => return Err(refused(format!("declares surfaces as {other:?}, not a mapping"))),
    };
    let mut basenames = BTreeSet::new();
    for (name, surface) in surfaces.into_iter().flatten() {
        let spec = match crate::gate::yaml_get(surface, "spec") {
            None => continue,
            Some(YamlValue::String(spec)) => spec,
            Some(other) => return Err(refused(format!("surface {name:?} declares spec as {other:?}, not text"))),
        };
        let basename = Path::new(spec)
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| refused(format!("surface {name:?} declares spec {spec:?}, which names no file")))?;
        basenames.insert(basename.to_string());
    }
    let mut spec_files = files_below(&harness.join("packages"), ".ts")?;
    spec_files.retain(|file| {
        file.file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| basenames.contains(name))
    });

    // Each referenced identifier with the spec file and line quoting it.
    let mut references: Vec<(String, JsonValue)> = Vec::new();
    let mut forbidden = Vec::new();
    let app_prefix = format!("{app_id}.");
    for file in &spec_files {
        let content = fs::read_to_string(file)?;
        for (value, offset) in quoted_values(&content) {
            if value.starts_with(&app_prefix) && valid_identifier(&value, true) {
                let at = json!({ "file": file.to_string_lossy(), "line": line_number(&content, offset) });
                references.push((value, at));
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

    // An identifier built by interpolation (`\(`) names no fixed identifier.
    let defined: BTreeSet<&str> = modifiers
        .iter()
        .map(|found| found.value.as_str())
        .filter(|value| !value.contains("\\("))
        .collect();
    let mut missing = Vec::new();
    let mut seen_missing = BTreeSet::new();
    for (value, at) in &references {
        if !defined.contains(value.as_str())
            && !prefixes.iter().any(|prefix| value.starts_with(prefix))
            && seen_missing.insert(value.as_str())
        {
            missing.push(json!({ "kind": "missing-identifier", "identifier": value, "referencedAt": at }));
        }
    }
    let mut locations: BTreeMap<(&str, &str), Vec<&Found>> = BTreeMap::new();
    for found in &modifiers {
        locations
            .entry((found.repository.as_str(), found.value.as_str()))
            .or_default()
            .push(found);
    }
    let mut duplicates = Vec::new();
    for ((_repository, identifier), entries) in locations {
        let files: BTreeSet<&str> = entries.iter().map(|found| found.file.as_str()).collect();
        if files.len() > 1 {
            let entries: Vec<JsonValue> = entries
                .iter()
                .map(|found| json!({ "file": found.file, "line": found.line }))
                .collect();
            duplicates.push(json!({ "kind": "duplicate-identifier", "identifier": identifier, "locations": entries }));
        }
    }
    let mut errors = duplicates;
    errors.extend(missing);
    errors.extend(forbidden);
    let referenced: BTreeSet<&str> = references.iter().map(|(value, _)| value.as_str()).collect();
    let unused: Vec<&str> = modifiers
        .iter()
        .map(|found| found.value.as_str())
        .filter(|identifier| identifier.starts_with(&app_prefix) && !referenced.contains(identifier))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(json!({
        "schemaVersion": REPORT_SCHEMA_VERSION,
        "appId": app_id,
        "ok": errors.is_empty(),
        "sourceFiles": scanned_files,
        "specFiles": spec_files.iter().map(|file| file.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "identifiers": { "defined": defined.len(), "explicit": modifiers.len(), "referenced": referenced.len(), "unused": unused },
        "errors": errors,
    }))
}

/// Validate and print the accessibility identifiers of `app_id`; a result
/// that is not ok is a refusal carrying it, never a process exit.
pub fn accessibility_command(harness: &Path, app_id: &str) -> crate::failure::Answer {
    print_result(
        "accessibility",
        "/ok",
        validate_accessibility(harness, app_id)?,
    )
}
