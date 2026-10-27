use crate::stado::*;
pub(crate) fn registration_directory(target: &str) -> Option<&'static str> {
    match target {
        "mobile:ios" | "mobile:android" => Some("packages/mobile/test/specs"),
        "desktop:mac" | "desktop:win" => Some("packages/desktop-native/test/specs"),
        "desktop:cua" => Some("packages/desktop-cua/specs"),
        "tui" => Some("packages/tui/specs"),
        _ => None,
    }
}

pub(crate) fn registration_extension(target: &str) -> &'static str {
    if matches!(target, "tui" | "desktop:cua") {
        ".spec.mjs"
    } else {
        ".e2e.ts"
    }
}

pub(crate) fn product_extension(target: &str) -> &'static str {
    if matches!(target, "tui" | "desktop:cua") {
        "mjs"
    } else {
        "ts"
    }
}

pub(crate) fn authored_path_is_inside(root: &Path, candidate: &Path) -> bool {
    candidate != root && candidate.starts_with(root)
}

pub(crate) fn relative_symlink_target(from: &Path, to: &Path) -> PathBuf {
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    // Paths that share no leading component have no relative route.
    match (from_components.first(), to_components.first()) {
        (Some(left), Some(right)) if left == right => {}
        _ => return to.to_path_buf(),
    }
    let shared = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for component in &from_components[shared..] {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &to_components[shared..] {
        relative.push(component.as_os_str());
    }
    relative
}

pub(crate) fn assert_physical_product_path(
    product_root: &Path,
    tests_root: &Path,
    product_spec: &Path,
) -> Result<(), Failure> {
    match fs::symlink_metadata(product_spec) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(Failure::config(
                "stado.download",
                "authored spec destination must be a regular product file",
            ));
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let parent = product_spec.parent().ok_or_else(|| {
        Failure::config(
            "stado.download",
            "authored spec path escapes the selected product tests directory",
        )
    })?;
    let mut existing_parent = parent;
    while !existing_parent.exists() {
        existing_parent = existing_parent.parent().ok_or_else(|| {
            Failure::config(
                "stado.download",
                "authored spec path escapes the selected product tests directory",
            )
        })?;
    }
    let physical_root = fs::canonicalize(product_root)?;
    let physical_parent = fs::canonicalize(existing_parent)?;
    if physical_parent != physical_root
        && !authored_path_is_inside(&physical_root, &physical_parent)
    {
        return Err(Failure::config(
            "stado.download",
            "authored spec path escapes the selected product tests directory",
        ));
    }
    if tests_root.exists() && parent.exists() {
        let physical_tests = fs::canonicalize(tests_root)?;
        let physical_product =
            fs::canonicalize(parent)?.join(product_spec.file_name().ok_or_else(|| {
                Failure::config(
                    "stado.download",
                    "authored spec path escapes the selected product tests directory",
                )
            })?);
        if !authored_path_is_inside(&physical_root, &physical_tests)
            || !authored_path_is_inside(&physical_tests, &physical_product)
        {
            return Err(Failure::config(
                "stado.download",
                "authored spec path escapes the selected product tests directory",
            ));
        }
    }
    Ok(())
}

pub(crate) fn install_product_spec(
    harness: &Path,
    product_root: &Path,
    app_id: &str,
    journey: &str,
    target: &str,
    product_relative: &str,
    registration_relative: &str,
    bytes: &[u8],
) -> Result<(PathBuf, PathBuf, PathBuf), Failure> {
    for relative in [product_relative, registration_relative] {
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(Failure::config(
                "stado.download",
                "Remote authoring returned an unsafe local installation path.",
            ));
        }
    }
    let loaded = manifest::load(harness, app_id)?;
    let manifest_file = loaded.file;
    let mut document = loaded.document;
    crate::manifest::declare_journey(&mut document, journey, "stado.download")?;
    let declared = document
        .get_mut("surfaces")
        .and_then(serde_yaml::Value::as_mapping_mut)
        .and_then(|surfaces| surfaces.get_mut(serde_yaml::Value::from(target)))
        .and_then(|surface| surface.get_mut("journeys"))
        .and_then(serde_yaml::Value::as_sequence_mut)
        .ok_or_else(|| {
            Failure::config(
                "stado.download",
                format!("app {app_id} has no {target} surface"),
            )
        })?;
    if !declared.iter().any(|value| value.as_str() == Some(journey)) {
        declared.push(serde_yaml::Value::from(journey));
        declared.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    }
    manifest::validate(&document, &manifest_file)?;

    let product_spec = product_root.join(product_relative);
    let tests_root = product_spec
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            Failure::config(
                "stado.download",
                "authored spec path escapes the selected product tests directory",
            )
        })?;
    assert_physical_product_path(product_root, tests_root, &product_spec)?;
    let registration = harness.join(registration_relative);
    let registration_parent = registration.parent().ok_or_else(|| {
        Failure::config(
            "stado.download",
            "the registration path has no parent directory",
        )
    })?;
    let replaced_product = replaced_product(registration_parent, &registration, &product_spec, tests_root)?;
    if let Some(parent) = product_spec.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(registration_parent)?;
    fs::write(&product_spec, bytes)?;
    if let Some(replaced) = replaced_product {
        fs::remove_file(replaced)?;
    }
    // A registration that cannot be examined is refused rather than read as
    // absent: the link below would then fail on a path nobody looked at.
    match fs::symlink_metadata(&registration) {
        Ok(_registered) => fs::remove_file(&registration)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let link_target = relative_symlink_target(registration_parent, &product_spec);
    std::os::unix::fs::symlink(link_target, &registration)?;
    fs::write(&manifest_file, serde_yaml::to_string(&document)?)?;
    Ok((product_spec, registration, manifest_file))
}

/// The product spec the existing registration links to, when it is a
/// different spec inside the product tests directory and so is replaced by
/// this install. A registration that is absent or not a link, or a link to
/// nothing, replaces nothing; one that cannot be read is refused, because
/// read as "nothing to replace" the old spec stayed behind beside the new.
fn replaced_product(
    registration_parent: &Path,
    registration: &Path,
    product_spec: &Path,
    tests_root: &Path,
) -> Result<Option<PathBuf>, Failure> {
    let Some(metadata) = examined(fs::symlink_metadata(registration))? else {
        return Ok(None);
    };
    if !metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let target = fs::read_link(registration)?;
    let candidate = match target.is_absolute() {
        true => target,
        false => registration_parent.join(target),
    };
    let Some(candidate) = examined(fs::canonicalize(candidate))? else {
        return Ok(None);
    };
    if examined(fs::canonicalize(product_spec))?.as_ref() == Some(&candidate) {
        return Ok(None);
    }
    let inside = examined(fs::canonicalize(tests_root))?
        .is_some_and(|tests| authored_path_is_inside(&tests, &candidate));
    Ok(inside.then_some(candidate))
}

/// A filesystem answer where absent is `None` and any other failure is the
/// caller's error.
fn examined<T>(answer: std::io::Result<T>) -> Result<Option<T>, Failure> {
    match answer {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
