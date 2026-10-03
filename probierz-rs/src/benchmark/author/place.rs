//! Where an authored suite or contender lands: a file in the product's own
//! tree and its declaration in the product's manifest.
//!
//! A contender driver lives at `benchmark/contenders/<id>/` for our product
//! and `benchmark/rivals/<id>/` for a rival, in the manifest's first
//! repository; the directory belongs to authoring, so a new draft replaces it
//! whole. A suite lives at `benchmark/<suite>.json` and is never overwritten:
//! a suite changes by a new id or version, because two runs compare only on
//! the same suite hash.

use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Value as Yaml};

use super::brief::Driver;
use crate::failure::Failure;
use crate::manifest::{self, Manifest};

fn root(manifest: &Manifest) -> Result<PathBuf, Failure> {
    manifest::primary_root(manifest).ok_or_else(|| {
        Failure::config(
            "benchmark.author",
            format!(
                "{} names no repository root, so there is no product tree to place the benchmark in",
                manifest.file.display()
            ),
        )
    })
}

/// A path as the manifest writes it: under the operator's home as `~/…`, so
/// the file names no account.
fn written(path: &Path) -> String {
    match std::env::var("HOME")
        .ok()
        .and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

fn section(document: &mut Yaml) -> Result<&mut Mapping, Failure> {
    let top = document
        .as_mapping_mut()
        .ok_or_else(|| Failure::config("benchmark.author", "the manifest is not a mapping"))?;
    let benchmark = top
        .entry(Yaml::from("benchmark"))
        .or_insert_with(|| Yaml::Mapping(Mapping::new()));
    benchmark
        .as_mapping_mut()
        .ok_or_else(|| Failure::config("benchmark.author", "benchmark is not a mapping"))
}

fn child<'a>(benchmark: &'a mut Mapping, key: &str) -> Result<&'a mut Mapping, Failure> {
    benchmark
        .entry(Yaml::from(key))
        .or_insert_with(|| Yaml::Mapping(Mapping::new()))
        .as_mapping_mut()
        .ok_or_else(|| {
            Failure::config(
                "benchmark.author",
                format!("benchmark.{key} is not a mapping"),
            )
        })
}

/// Rewrite the manifest with one change to its `benchmark` section, read
/// fresh so a concurrent edit elsewhere in the file is kept.
fn edit(
    manifest: &Manifest,
    change: impl FnOnce(&mut Mapping) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let text = std::fs::read_to_string(&manifest.file)?;
    let mut document: Yaml = serde_yaml::from_str(&text)?;
    change(section(&mut document)?)?;
    std::fs::write(&manifest.file, serde_yaml::to_string(&document)?)?;
    Ok(())
}

/// The suite file the manifest declares for `suite_id`, read without
/// requiring any contender to be declared yet.
pub(crate) fn suite_file(manifest: &Manifest, suite_id: &str) -> Result<PathBuf, Failure> {
    let written = manifest
        .document
        .get("benchmark")
        .and_then(|benchmark| benchmark.get("suites"))
        .and_then(|suites| suites.get(suite_id))
        .and_then(Yaml::as_str)
        .ok_or_else(|| {
            Failure::invalid(
                "benchmark.author",
                format!(
                    "{} declares no suite {suite_id}; probierz benchmark author-suite {} --suite {suite_id} drafts one",
                    manifest.file.display(),
                    manifest.app_id
                ),
            )
        })?;
    let path = Path::new(written);
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest
            .file
            .parent()
            .expect("a manifest sits in its app directory")
            .join(path)
    })
}

/// The contender the manifest declares as ours, if any.
pub(crate) fn ours(manifest: &Manifest) -> Option<String> {
    let contenders = manifest
        .document
        .get("benchmark")?
        .get("contenders")?
        .as_mapping()?;
    contenders.iter().find_map(|(id, declaration)| {
        (declaration.get("ours").and_then(Yaml::as_bool) == Some(true))
            .then(|| id.as_str().map(str::to_string))
            .flatten()
    })
}

/// The program a declared contender runs, if it is declared.
pub(crate) fn program(manifest: &Manifest, id: &str) -> Option<PathBuf> {
    let declared = manifest
        .document
        .get("benchmark")?
        .get("contenders")?
        .get(id)?;
    declared
        .get("program")
        .and_then(Yaml::as_str)
        .map(PathBuf::from)
}

/// Write one drafted driver, replacing whatever an earlier draft left.
pub(crate) fn driver(
    manifest: &Manifest,
    id: &str,
    ours: bool,
    driver: &Driver,
) -> Result<PathBuf, Failure> {
    let side = if ours { "contenders" } else { "rivals" };
    let directory = root(manifest)?.join("benchmark").join(side).join(id);
    if directory.exists() {
        std::fs::remove_dir_all(&directory)?;
    }
    std::fs::create_dir_all(&directory)?;
    let file = directory.join(&driver.file);
    std::fs::write(&file, &driver.source)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(file)
}

/// Declare a contender, replacing an earlier declaration of the same id.
pub(crate) fn declare(
    manifest: &Manifest,
    id: &str,
    ours: bool,
    program: &Path,
    env: &[String],
) -> Result<(), Failure> {
    edit(manifest, |benchmark| {
        let mut declaration = Mapping::new();
        if ours {
            declaration.insert(Yaml::from("ours"), Yaml::from(true));
        }
        declaration.insert(Yaml::from("program"), Yaml::from(written(program)));
        declaration.insert(
            Yaml::from("env"),
            Yaml::Sequence(env.iter().map(|name| Yaml::from(name.as_str())).collect()),
        );
        child(benchmark, "contenders")?.insert(Yaml::from(id), Yaml::Mapping(declaration));
        Ok(())
    })
}

/// Where a drafted suite is written; refused while a file is already there.
pub(crate) fn suite_target(manifest: &Manifest, suite_id: &str) -> Result<PathBuf, Failure> {
    let file = root(manifest)?
        .join("benchmark")
        .join(format!("{suite_id}.json"));
    if file.exists() {
        return Err(Failure::invalid(
            "benchmark.author",
            format!(
                "{} already holds suite {suite_id}; a suite changes under a new id or version, never in place, because runs compare only on the same suite hash",
                file.display()
            ),
        ));
    }
    Ok(file)
}

/// Declare a suite file under its id.
pub(crate) fn declare_suite(
    manifest: &Manifest,
    suite_id: &str,
    file: &Path,
) -> Result<(), Failure> {
    edit(manifest, |benchmark| {
        child(benchmark, "suites")?.insert(Yaml::from(suite_id), Yaml::from(written(file)));
        child(benchmark, "contenders")?;
        Ok(())
    })
}
