use crate::gate::*;

/// What git prints on stdout for `args` in `repo`, or why it printed nothing
/// to read: git that cannot start or exits non-zero is the cause, with its
/// exit and stderr.
fn git_stdout(repo: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = ProcessCommand::new("git");
    command.arg("-C").arg(repo).args(args);
    let output = stado_wait::output(&mut command).map_err(|error| {
        format!(
            "git {} in {} could not run: {error}",
            args.join(" "),
            repo.display()
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "git {} in {} {}: {}",
            args.join(" "),
            repo.display(),
            crate::failure::ended(&output.status),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The one value git prints (a revision, a merge base), or why there is
/// none: read as absent, the gate said "cannot resolve" without git's own
/// reason.
pub(crate) fn revision(repo: &Path, args: &[&str]) -> Result<String, String> {
    let text = git_stdout(repo, args)?;
    match text.trim() {
        "" => Err(format!(
            "git {} in {} printed nothing",
            args.join(" "),
            repo.display()
        )),
        value => Ok(value.to_string()),
    }
}

/// The non-empty lines git prints, or why it printed none: git that cannot
/// start or exits non-zero is the cause, never an empty answer. An empty list
/// read as "nothing changed" let the gate pass on a diff it never saw.
pub(crate) fn git_lines(repo: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    Ok(git_stdout(repo, args)?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

pub(crate) fn manifest_repositories(app: &manifest::Manifest) -> Vec<&Yaml> {
    match yaml_get(&app.document, "repositories").and_then(Yaml::as_sequence) {
        Some(list) => list.iter().collect(),
        None => Vec::new(),
    }
}

/// A path resolved for comparison, or the gate's refusal naming why it
/// cannot be.
fn resolved(path: &Path) -> Result<PathBuf, Failure> {
    normalize_absolute(path).map_err(|cause| Failure::config("gate.paths", cause))
}

pub(crate) fn infer_app_id(harness: &Path, repo: &Path) -> Result<Option<String>, Failure> {
    let normalized = resolved(repo)?;
    for summary in manifest::list(harness)? {
        let app = manifest::load(harness, &summary.app_id)?;
        for repository in manifest_repositories(&app) {
            let Some(root) = yaml_string(yaml_get(repository, "root")) else {
                continue;
            };
            if resolved(Path::new(&root))? == normalized {
                return Ok(Some(app.app_id));
            }
        }
    }
    Ok(None)
}

/// A list a mapping declares under `field`: absent is none; anything but a
/// list of strings is refused naming the app, so a malformed mapping never
/// reads as one that maps nothing and lets every change through.
fn declared_list(
    app: &manifest::Manifest,
    holder: &Yaml,
    field: &str,
) -> Result<Vec<String>, Failure> {
    let refused =
        |what: String| Failure::config("gate.mappings", format!("app {} {what}", app.app_id));
    match yaml_get(holder, field) {
        None | Some(Yaml::Null) => Ok(Vec::new()),
        Some(Yaml::Sequence(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_string).ok_or_else(|| {
                    refused(format!("declares {field} holding {item:?}, not a string"))
                })
            })
            .collect(),
        Some(other) => Err(refused(format!(
            "declares {field} as {other:?}, not a list"
        ))),
    }
}

/// The journeys `files` touch in `app`, read from each repository's
/// `mappings`: every path pattern a changed file under the repository's root
/// matches names its journeys. A repository without a root, or a mapping
/// whose `mappings`, `paths` or `journeys` is not a list, is refused naming
/// the app: skipped, its changes mapped to no journey and the push or merge
/// was judged on none of them. The pre-push gate and `probierz status` both
/// read journeys here.
pub(crate) fn affected_journeys(
    app: &manifest::Manifest,
    files: &[PathBuf],
) -> Result<Vec<String>, Failure> {
    let mut journeys = BTreeSet::new();
    for repository in manifest_repositories(app) {
        let root = yaml_string(yaml_get(repository, "root")).ok_or_else(|| {
            Failure::config(
                "gate.mappings",
                format!("app {} declares a repository without root", app.app_id),
            )
        })?;
        let root = resolved(Path::new(&root))?;
        let mappings = match yaml_get(repository, "mappings") {
            None | Some(Yaml::Null) => Vec::new(),
            Some(Yaml::Sequence(mappings)) => mappings.iter().collect(),
            Some(other) => {
                return Err(Failure::config(
                    "gate.mappings",
                    format!(
                        "app {} declares mappings as {other:?}, not a list",
                        app.app_id
                    ),
                ))
            }
        };
        for mapping in mappings {
            let patterns = declared_list(app, mapping, "paths")?;
            let named = declared_list(app, mapping, "journeys")?;
            for file in files {
                let file = resolved(file)?;
                // A file outside this repository's root is another
                // repository's change.
                if !file.starts_with(&root) {
                    continue;
                }
                let relative = file
                    .components()
                    .skip(root.components().count())
                    .collect::<PathBuf>()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                if patterns
                    .iter()
                    .any(|pattern| crate::run::glob_matches(pattern, &relative))
                {
                    journeys.extend(named.iter().cloned());
                }
            }
        }
    }
    Ok(journeys.into_iter().collect())
}
