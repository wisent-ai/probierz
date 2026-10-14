use crate::run::*;
use serde_json::json;
pub(crate) fn build_identity(
    harness: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    let candidate = [
        "PROBIERZ_BUILD_PATH",
        "APP_IOS",
        "MAC_APP_PATH",
        "ELECTRON_APP_MAIN",
    ]
    .iter()
    .find_map(|name| env.get(*name).cloned())
    .unwrap_or_else(|| {
        harness
            .join("package-lock.json")
            .to_string_lossy()
            .into_owned()
    });
    let candidate_path = Path::new(&candidate);
    let resolved = if candidate_path.is_absolute() {
        candidate_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(candidate_path)
    };
    let path = normalize_path(&resolved);
    let sha = if path.is_file() {
        Some(crate::evidence::sha256_file(&path)?)
    } else if path.is_dir() {
        let mut hash = Sha256::new();
        for file in walk(&path, true) {
            hash.update(
                file.strip_prefix(&path)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .as_bytes(),
            );
            hash.update(fs::read(file)?);
        }
        Some(hex::encode(hash.finalize()))
    } else {
        None
    };
    Ok(json!({ "path": path, "sha256": sha }))
}
