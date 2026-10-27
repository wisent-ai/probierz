//! What changed: the journeys a set of files affects, read through the git and
//! path-pattern readers the pre-push gate and run planning share.

use crate::status::*;

/// The checkout a manifest repository declares. A repository without one is
/// refused by name, never measured in whatever directory the process runs in.
pub(crate) fn repository_root<'a>(app_id: &str, repository: &'a Value) -> Result<&'a str, Failure> {
    repository
        .get("root")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Failure::config(
                "status.repository",
                format!("app {app_id} declares a repository without root: {repository}"),
            )
        })
}

/// The journeys `files` touch across every app of the harness, each app's
/// mappings read by the reader the pre-push gate uses
/// (`gate::affected_journeys`), so a merge and a push are judged on the same
/// journeys and a malformed mapping refuses both.
pub(crate) fn affected_journeys(harness: &Path, files: &[PathBuf]) -> Result<Vec<String>, Failure> {
    let mut affected = BTreeSet::new();
    for app in manifest::list(harness)? {
        let loaded = manifest::load(harness, &app.app_id)?;
        affected.extend(crate::gate::affected_journeys(&loaded, files)?);
    }
    Ok(affected.into_iter().collect())
}
