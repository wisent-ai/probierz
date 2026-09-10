use crate::readme_gif::*;
pub(crate) fn invalid(detail: impl Into<String>) -> Failure {
    Failure::invalid("readme-gif", detail)
}

pub(crate) fn bounded(value: f64, name: &str, maximum: f64) -> Result<f64, Failure> {
    if !value.is_finite() || value < 1.0 || value > maximum {
        return Err(invalid(format!("{name} must be between 1 and {maximum}")));
    }
    Ok(value)
}

pub(crate) fn non_negative(value: f64, name: &str) -> Result<f64, Failure> {
    if !value.is_finite() || value < 0.0 {
        return Err(invalid(format!("{name} must be a non-negative number")));
    }
    Ok(value)
}

pub(crate) fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    std::path::absolute(path).map_err(|error| invalid(error.to_string()))
}

pub(crate) fn regular_file(file: &Path, label: &str) -> Result<PathBuf, Failure> {
    let resolved = absolute(file)?;
    let metadata = match fs::symlink_metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(invalid(format!(
                "{label} does not exist: {}",
                file.display()
            )));
        }
        Err(error) => {
            return Err(Failure::new(
                "readme-gif.input",
                crate::failure::Code::Config,
                error.to_string(),
            ))
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid(format!(
            "{label} must be a regular, non-symlink file: {}",
            file.display()
        )));
    }
    Ok(resolved)
}

pub(crate) fn output_file(file: &Path) -> Result<PathBuf, Failure> {
    let resolved = absolute(file)?;
    let is_gif = resolved
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"));
    if !is_gif {
        return Err(invalid("--out must end in .gif"));
    }
    Ok(resolved)
}

pub(crate) fn sha256(file: &Path) -> Result<String, Failure> {
    let mut input = File::open(file)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

pub(crate) fn json_number(value: f64) -> Value {
    if value.fract() == 0.0 && value <= u64::MAX as f64 {
        Value::Number((value as u64).into())
    } else {
        Value::Number(Number::from_f64(value).expect("validated finite number"))
    }
}

pub(crate) fn filter_graph(frames_per_second: f64, width: f64) -> String {
    format!(
        "[0:v]fps={frames_per_second},scale={width}:-2:flags=lanczos,split[v0][v1];[v0]palettegen=stats_mode=diff[p];[v1][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle[v]"
    )
}

