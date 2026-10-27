use crate::run::*;
use serde_json::json;
/// The build a run tested: the first of `PROBIERZ_BUILD_PATH`, `APP_IOS`,
/// `MAC_APP_PATH` and `ELECTRON_APP_MAIN` the run's environment declares, or,
/// when it declares none, the harness's `package-lock.json` (a web harness's
/// build is its locked dependency set). Its path and the SHA-256 of its
/// bytes, or of every file under it with each file's path inside it; a path
/// that does not exist has no digest.
pub(crate) fn build_identity(
    harness: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    let declared = [
        "PROBIERZ_BUILD_PATH",
        "APP_IOS",
        "MAC_APP_PATH",
        "ELECTRON_APP_MAIN",
    ]
    .iter()
    .find_map(|name| env.get(*name).cloned());
    let candidate = match declared {
        Some(declared) => declared,
        None => harness
            .join("package-lock.json")
            .to_string_lossy()
            .into_owned(),
    };
    let candidate_path = Path::new(&candidate);
    let resolved = if candidate_path.is_absolute() {
        candidate_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(candidate_path)
    };
    let path = normalize_path(&resolved);
    // A build that cannot be examined is refused: read as absent, the report
    // carried no digest for it, as if no build had been named.
    let sha = if !crate::paths::present(&path)? {
        None
    } else if path.is_file() {
        Some(crate::evidence::sha256_file(&path)?)
    } else if path.is_dir() {
        let mut hash = Sha256::new();
        for file in walk(&path, true) {
            let inside = file.strip_prefix(&path).map_err(|error| {
                Failure::invalid(
                    "run.report.build_identity",
                    format!(
                        "{} was listed under the build {} but lies outside it: {error}",
                        file.display(),
                        path.display()
                    ),
                )
            })?;
            hash.update(inside.to_string_lossy().as_bytes());
            hash.update(fs::read(&file)?);
        }
        Some(hex::encode(hash.finalize()))
    } else {
        None
    };
    Ok(json!({ "path": path, "sha256": sha }))
}
