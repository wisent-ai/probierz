//! What changed: git reads, glob matching and the journeys a set of files affects.

use super::*;

pub(super) fn git(root: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

pub(super) fn git_lines(root: &str, arguments: &[&str]) -> Vec<String> {
    let Some(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .ok()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn glob_matches(pattern: &str, text: &str) -> bool {
    fn matches(
        pattern: &[u8],
        text: &[u8],
        memo: &mut HashMap<(usize, usize), bool>,
        pi: usize,
        ti: usize,
    ) -> bool {
        if let Some(answer) = memo.get(&(pi, ti)) {
            return *answer;
        }
        let answer = if pi == pattern.len() {
            ti == text.len()
        } else if pattern[pi] == b'*' && pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
            matches(pattern, text, memo, pi + 2, ti)
                || (ti < text.len() && matches(pattern, text, memo, pi, ti + 1))
        } else if pattern[pi] == b'*' {
            matches(pattern, text, memo, pi + 1, ti)
                || (ti < text.len() && text[ti] != b'/' && matches(pattern, text, memo, pi, ti + 1))
        } else {
            ti < text.len()
                && pattern[pi] == text[ti]
                && matches(pattern, text, memo, pi + 1, ti + 1)
        };
        memo.insert((pi, ti), answer);
        answer
    }
    matches(
        pattern.as_bytes(),
        text.as_bytes(),
        &mut HashMap::new(),
        0,
        0,
    )
}

pub(super) fn affected_journeys(harness: &Path, files: &[PathBuf]) -> Result<Vec<String>, Failure> {
    let mut affected = BTreeSet::new();
    for app in manifest::list(harness)? {
        let (_, document) = manifest_object(harness, &app.app_id)?;
        for repository in document
            .get("repositories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(root) = repository.get("root").and_then(Value::as_str) else {
                continue;
            };
            let root_path = Path::new(root);
            for file in files {
                let Ok(relative) = file.strip_prefix(root_path) else {
                    continue;
                };
                let relative = relative
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                for mapping in repository
                    .get("mappings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let matched = mapping
                        .get("paths")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .any(|pattern| glob_matches(pattern, &relative));
                    if matched {
                        for journey in mapping
                            .get("journeys")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            if let Some(journey) = journey.as_str() {
                                affected.insert(journey.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(affected.into_iter().collect())
}
