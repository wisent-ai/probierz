use crate::gate::*;
pub(crate) fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub(crate) fn git_lines(repo: &Path, args: &[&str]) -> Vec<String> {
    let Some(text) = git(repo, args) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn manifest_repositories(app: &manifest::Manifest) -> Vec<&Yaml> {
    yaml_get(&app.document, "repositories")
        .and_then(Yaml::as_sequence)
        .map(|list| list.iter().collect())
        .unwrap_or_default()
}

pub(crate) fn infer_app_id(harness: &Path, repo: &Path) -> Result<Option<String>, Failure> {
    let normalized = normalize_absolute(repo);
    for summary in manifest::list(harness)? {
        let app = manifest::load(harness, &summary.app_id)?;
        if manifest_repositories(&app).iter().any(|repository| {
            yaml_string(yaml_get(repository, "root"))
                .map(|root| normalize_absolute(Path::new(&root)) == normalized)
                .unwrap_or(false)
        }) {
            return Ok(Some(app.app_id));
        }
    }
    Ok(None)
}

pub(crate) fn glob_matches(pattern: &str, text: &str) -> bool {
    fn matches(
        pattern: &[u8],
        text: &[u8],
        pi: usize,
        ti: usize,
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(result) = memo.get(&(pi, ti)) {
            return *result;
        }
        let result = if pi == pattern.len() {
            ti == text.len()
        } else if pattern[pi] == b'*' && pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
            matches(pattern, text, pi + 2, ti, memo)
                || (ti < text.len() && matches(pattern, text, pi, ti + 1, memo))
        } else if pattern[pi] == b'*' {
            matches(pattern, text, pi + 1, ti, memo)
                || (ti < text.len() && text[ti] != b'/' && matches(pattern, text, pi, ti + 1, memo))
        } else {
            ti < text.len()
                && pattern[pi] == text[ti]
                && matches(pattern, text, pi + 1, ti + 1, memo)
        };
        memo.insert((pi, ti), result);
        result
    }
    matches(
        pattern.as_bytes(),
        text.as_bytes(),
        0,
        0,
        &mut HashMap::new(),
    )
}

pub(crate) fn affected_journeys(app: &manifest::Manifest, files: &[PathBuf]) -> Vec<String> {
    let mut journeys = BTreeSet::new();
    for repository in manifest_repositories(app) {
        let Some(root) = yaml_string(yaml_get(repository, "root")) else {
            continue;
        };
        let root = normalize_absolute(Path::new(&root));
        let mappings = yaml_get(repository, "mappings")
            .and_then(Yaml::as_sequence)
            .cloned()
            .unwrap_or_default();
        for file in files {
            let file = normalize_absolute(file);
            let Ok(relative) = file.strip_prefix(&root) else {
                continue;
            };
            let relative = relative
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            for mapping in &mappings {
                let patterns = yaml_strings(yaml_get(mapping, "paths"));
                if patterns
                    .iter()
                    .any(|pattern| glob_matches(pattern, &relative))
                {
                    journeys.extend(yaml_strings(yaml_get(mapping, "journeys")));
                }
            }
        }
    }
    journeys.into_iter().collect()
}

