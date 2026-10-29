//! The test gates a checkout's release manifest declares for this platform.
//!
//! `.wisent-release.json` lists, under `platforms.<name>.quality`, the gates
//! a release of that platform passes; a gate of kind `test` runs one of the
//! product's own suites. The manifest's platform names are the release
//! pipeline's vocabulary (`darwin-arm64`, `linux-x64`), so the one running
//! this binary is named in those words from the compiler's own constants.

use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

use crate::failure::Failure;

use super::{MANIFEST_FILE, TEST_KIND};

/// One suite as the manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestGate {
    pub(crate) platform: String,
    pub(crate) name: String,
    pub(crate) argv: Vec<String>,
    /// Paths relative to the checkout the suite leaves its reports at.
    pub(crate) reports: Vec<String>,
}

/// The gates one run executes, with the product they belong to.
pub(crate) struct Selected {
    pub(crate) product: String,
    pub(crate) gates: Vec<TestGate>,
}

/// This host in the release pipeline's platform vocabulary.
pub(crate) fn host_platform() -> Result<String, Failure> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => {
            return Err(Failure::config(
                "suites.platform",
                format!("no release platform is declared for operating system {other:?}"),
            ))
        }
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => {
            return Err(Failure::config(
                "suites.platform",
                format!("no release platform is declared for architecture {other:?}"),
            ))
        }
    };
    Ok(format!("{os}-{arch}"))
}

fn manifest_path(repo: &Path) -> PathBuf {
    repo.join(MANIFEST_FILE)
}

fn read_manifest(repo: &Path) -> Result<Value, Failure> {
    let path = manifest_path(repo);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Failure::config(
            "suites.manifest",
            format!("{} could not be read: {error}", path.display()),
        )
    })?;
    serde_json::from_str(&text).map_err(|error| {
        Failure::config(
            "suites.manifest",
            format!("{} is not JSON: {error}", path.display()),
        )
    })
}

/// The product the manifest names.
pub(crate) fn product_name(repo: &Path) -> Result<String, Failure> {
    let manifest = read_manifest(repo)?;
    match manifest.get("product").and_then(Value::as_str) {
        Some(product) if !product.trim().is_empty() => Ok(product.to_string()),
        _ => Err(Failure::config(
            "suites.manifest",
            format!("{} declares no product name", manifest_path(repo).display()),
        )),
    }
}

fn string_list(value: &Value, what: &str, gate: &str, path: &Path) -> Result<Vec<String>, Failure> {
    let items = value.as_array().ok_or_else(|| {
        Failure::config(
            "suites.manifest",
            format!(
                "{}: gate {gate:?} declares {what} as {value} where a list of strings is expected",
                path.display()
            ),
        )
    })?;
    items
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                Failure::config(
                    "suites.manifest",
                    format!(
                        "{}: gate {gate:?} declares {what} entry {item} where a string is expected",
                        path.display()
                    ),
                )
            })
        })
        .collect()
}

fn checkout_relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Every gate of kind `test` the manifest declares for this platform.
pub(crate) fn test_gates(repo: &Path) -> Result<Vec<TestGate>, Failure> {
    let manifest = read_manifest(repo)?;
    let path = manifest_path(repo);
    let platform = host_platform()?;
    let Some(platforms) = manifest.get("platforms").and_then(Value::as_object) else {
        return Err(Failure::config(
            "suites.manifest",
            format!("{} declares no platforms", path.display()),
        ));
    };
    let Some(declared) = platforms.get(&platform) else {
        return Err(Failure::config(
            "suites.manifest",
            format!(
                "{} declares no platform {platform}; declared: {}",
                path.display(),
                platforms.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        ));
    };
    let Some(quality) = declared.get("quality").and_then(Value::as_array) else {
        return Err(Failure::config(
            "suites.manifest",
            format!(
                "{} declares no quality gates for platform {platform}",
                path.display()
            ),
        ));
    };
    let mut gates = Vec::new();
    for gate in quality {
        if gate.get("kind").and_then(Value::as_str) != Some(TEST_KIND) {
            continue;
        }
        let name = gate
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| {
                Failure::config(
                    "suites.manifest",
                    format!(
                        "{}: a test gate of platform {platform} has no name: {gate}",
                        path.display()
                    ),
                )
            })?;
        let mut name_parts = Path::new(name).components();
        name_parts.next();
        if !checkout_relative(name) || name_parts.next().is_some() {
            return Err(Failure::config(
                "suites.manifest",
                format!(
                    "{}: gate {name:?} must have a single path-safe name",
                    path.display()
                ),
            ));
        }
        if gates
            .iter()
            .any(|existing: &TestGate| existing.name == name)
        {
            return Err(Failure::config(
                "suites.manifest",
                format!(
                    "{}: gate {name:?} is declared more than once",
                    path.display()
                ),
            ));
        }
        let argv = match gate.get("argv") {
            Some(argv) => string_list(argv, "argv", name, &path)?,
            None => {
                return Err(Failure::config(
                    "suites.manifest",
                    format!("{}: gate {name:?} declares no argv", path.display()),
                ))
            }
        };
        if argv.is_empty() {
            return Err(Failure::config(
                "suites.manifest",
                format!("{}: gate {name:?} declares an empty argv", path.display()),
            ));
        }
        let reports = match gate.get("reports") {
            Some(reports) => string_list(reports, "reports", name, &path)?,
            None => Vec::new(),
        };
        for report in &reports {
            if !checkout_relative(report) {
                return Err(Failure::config(
                    "suites.manifest",
                    format!(
                        "{}: gate {name:?} report {report:?} must be checkout-relative without parent traversal",
                        path.display()
                    ),
                ));
            }
        }
        gates.push(TestGate {
            platform: platform.clone(),
            name: name.to_string(),
            argv,
            reports,
        });
    }
    Ok(gates)
}

/// The gates named by `--gate`, or every declared one; a name nothing
/// declares is refused with the declared names.
pub(crate) fn select(
    repo: &Path,
    declared: Vec<TestGate>,
    named: &[String],
) -> Result<Selected, Failure> {
    let product = product_name(repo)?;
    if declared.is_empty() {
        return Err(Failure::config(
            "suites.select",
            format!(
                "{} declares no test gate for this platform; declare each suite as a quality gate of kind {TEST_KIND:?}",
                manifest_path(repo).display()
            ),
        ));
    }
    let gates = if named.is_empty() {
        declared
    } else {
        let mut chosen = Vec::new();
        for name in named {
            match declared.iter().find(|gate| &gate.name == name) {
                Some(gate) => chosen.push(gate.clone()),
                None => {
                    return Err(Failure::invalid(
                        "suites.select",
                        format!(
                            "no test gate named {name:?}; declared: {}",
                            declared
                                .iter()
                                .map(|gate| gate.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ))
                }
            }
        }
        chosen
    };
    Ok(Selected { product, gates })
}

/// The listing `suites list` prints.
pub(crate) fn listing(repo: &Path, gates: &[TestGate]) -> Value {
    json!({
        "repository": repo,
        "manifest": manifest_path(repo),
        "suites": gates.iter().map(|gate| json!({
            "platform": gate.platform,
            "name": gate.name,
            "argv": gate.argv,
            "reports": gate.reports,
        })).collect::<Vec<_>>(),
    })
}
