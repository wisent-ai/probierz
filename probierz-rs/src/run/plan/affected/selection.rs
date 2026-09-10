use serde_json::json;
use crate::run::*;
pub(crate) fn affected_app_journeys(harness: &Path, files: &[String]) -> Result<Vec<Value>, Failure> {
    let mut matches = Vec::new();
    for app in manifest::list(harness)? {
        let declaration = manifest::load(harness, &app.app_id)?;
        let document = &declaration.document;
        let mut journey_targets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        if let Some(surfaces) = document
            .get("surfaces")
            .and_then(serde_yaml::Value::as_mapping)
        {
            for (target, surface) in surfaces {
                let Some(target) = target.as_str() else {
                    continue;
                };
                for journey in surface
                    .get("journeys")
                    .and_then(serde_yaml::Value::as_sequence)
                    .into_iter()
                    .flatten()
                    .filter_map(serde_yaml::Value::as_str)
                {
                    journey_targets
                        .entry(journey.into())
                        .or_default()
                        .insert(target.into());
                }
            }
        }
        for repository in document
            .get("repositories")
            .and_then(serde_yaml::Value::as_sequence)
            .into_iter()
            .flatten()
        {
            let Some(root) = repository.get("root").and_then(serde_yaml::Value::as_str) else {
                continue;
            };
            let root_path = Path::new(root);
            for input in files {
                let input_path = Path::new(input);
                let absolute = if input_path.is_absolute() {
                    normalize_path(input_path)
                } else {
                    normalize_path(&root_path.join(input_path))
                };
                let Ok(relative) = absolute.strip_prefix(root_path) else {
                    continue;
                };
                let relative = slash(relative);
                for mapping in repository
                    .get("mappings")
                    .and_then(serde_yaml::Value::as_sequence)
                    .into_iter()
                    .flatten()
                {
                    let patterns: Vec<&str> = mapping
                        .get("paths")
                        .and_then(serde_yaml::Value::as_sequence)
                        .into_iter()
                        .flatten()
                        .filter_map(serde_yaml::Value::as_str)
                        .collect();
                    if !patterns
                        .iter()
                        .any(|pattern| glob_matches(pattern, &relative))
                    {
                        continue;
                    }
                    let journeys: Vec<String> = mapping
                        .get("journeys")
                        .and_then(serde_yaml::Value::as_sequence)
                        .into_iter()
                        .flatten()
                        .filter_map(serde_yaml::Value::as_str)
                        .map(str::to_string)
                        .collect();
                    let targets: BTreeSet<String> = journeys
                        .iter()
                        .flat_map(|journey| {
                            journey_targets.get(journey).into_iter().flatten().cloned()
                        })
                        .collect();
                    matches.push(json!({ "appId": app.app_id, "input": input, "file": absolute.to_string_lossy(), "repository": root, "journeys": journeys, "targets": targets }));
                }
            }
        }
    }
    Ok(matches)
}

pub(crate) fn path_inside(parent: &Path, child: &Path) -> bool {
    normalize_path(child)
        .strip_prefix(normalize_path(parent))
        .is_ok()
}

pub(crate) fn affected_targets(harness: &Path, files: &[String]) -> Result<Value, Failure> {
    let app_matches = affected_app_journeys(harness, files)?;
    let mut by_package: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for name in target_list() {
        by_package
            .entry(target(name).expect("known").pkg)
            .or_default()
            .push(name);
    }
    let mut hit = BTreeSet::new();
    let mut cross_cutting = false;
    let mut classified = Vec::new();
    for raw in files {
        let product: Vec<&Value> = app_matches
            .iter()
            .filter(|entry| entry.get("input").and_then(Value::as_str) == Some(raw))
            .collect();
        if !product.is_empty() {
            let targets: BTreeSet<String> = product
                .iter()
                .flat_map(|entry| {
                    entry
                        .get("targets")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                })
                .collect();
            hit.extend(targets.clone());
            let apps: Vec<Value> = product.iter().map(|entry| json!({ "appId": entry["appId"], "journeys": entry["journeys"], "repository": entry["repository"] })).collect();
            classified.push(json!({ "file": slash(&normalize_path(Path::new(raw))), "affects": targets, "apps": apps }));
            continue;
        }
        let normalized = normalize_path(Path::new(raw));
        let package = by_package
            .iter()
            .find(|(pkg, _)| path_inside(Path::new(pkg), &normalized));
        if let Some((_pkg, names)) = package {
            for name in names {
                hit.insert((*name).to_string());
            }
            classified.push(json!({ "file": slash(&normalized), "affects": names }));
        } else if path_inside(Path::new("agent"), &normalized)
            || normalized.parent() == Some(Path::new(""))
        {
            cross_cutting = true;
            classified.push(json!({ "file": slash(&normalized), "affects": "all" }));
        } else {
            classified.push(json!({ "file": slash(&normalized), "affects": [] }));
        }
    }
    let targets: Vec<String> = if cross_cutting {
        target_list()
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    } else {
        hit.into_iter().collect()
    };
    let apps: Vec<Value> = app_matches
        .into_iter()
        .map(|mut entry| {
            entry.as_object_mut().expect("object").shift_remove("input");
            entry
        })
        .collect();
    Ok(
        json!({ "targets": targets, "crossCutting": cross_cutting, "files": classified, "apps": apps }),
    )
}

pub(crate) fn changed_files(harness: &Path, reference: &str) -> Result<Vec<String>, Failure> {
    let result = capture(
        "git",
        &[
            "-C".into(),
            harness.to_string_lossy().into_owned(),
            "diff".into(),
            "--name-only".into(),
            reference.into(),
        ],
        None,
        None,
        None,
    );
    if !result.status.is_some_and(|status| status.success()) {
        return Err(fail(
            "run.affected",
            format!(
                "git diff --name-only {reference} failed: {}",
                text(&result.stderr).trim()
            ),
        ));
    }
    Ok(text(&result.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

pub(crate) fn affected_from_git(harness: &Path, reference: Option<&str>) -> Result<Value, Failure> {
    let against = reference.unwrap_or("HEAD");
    let files = changed_files(harness, against)?;
    let mut result = affected_targets(harness, &files)?;
    let mut object = Map::new();
    object.insert("ref".into(), Value::String(against.into()));
    object.extend(result.as_object_mut().expect("object").clone());
    Ok(Value::Object(object))
}

pub fn affected(harness: &Path, args: &[String]) -> Answer {
    let result = if let Some(files) = files_after_flag(args) {
        if files.is_empty() {
            return Err(fail("cli.affected", "--files needs at least one path"));
        }
        affected_targets(harness, &files)?
    } else {
        let reference = args
            .first()
            .filter(|arg| !arg.starts_with("--"))
            .map(String::as_str);
        affected_from_git(harness, reference)?
    };
    print_json(&result)
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glob_double_star_crosses_directories_but_star_does_not() {
        assert!(glob_matches("src/**/view.ts", "src/a/b/view.ts"));
        assert!(!glob_matches("src/*/view.ts", "src/a/b/view.ts"));
    }
}
