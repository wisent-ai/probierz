use crate::specs::*;
pub(crate) fn write_report(path: &Path, report: &Value) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| fail("specs.report", format!("{}: {error}", parent.display())))?;
    }
    let mut body = serde_json::to_vec_pretty(report)
        .map_err(|error| fail("specs.report", error.to_string()))?;
    body.push(b'\n');
    let mut file = create_private(path)
        .map_err(|error| fail("specs.report", format!("{}: {error}", path.display())))?;
    file.write_all(&body)
        .map_err(|error| fail("specs.report", format!("{}: {error}", path.display())))?;
    Ok(())
}
