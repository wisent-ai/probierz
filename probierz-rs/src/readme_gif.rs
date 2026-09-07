//! Publication of a recorded journey as a bounded README GIF.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};

use crate::failure::{print_json, write_private, Answer, Failure};

const MAX_DURATION_SECONDS: f64 = 30.0;
const MAX_FRAMES_PER_SECOND: f64 = 20.0;
const MAX_WIDTH: f64 = 1200.0;

#[derive(Debug, Clone)]
pub struct Options {
    pub input: PathBuf,
    pub output: PathBuf,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub frames_per_second: f64,
    pub width: f64,
    pub force: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Render {
    start_seconds: Value,
    duration_seconds: Value,
    frames_per_second: Value,
    width: Value,
    silent: bool,
    r#loop: bool,
}

#[derive(Serialize)]
struct FileHash {
    file: String,
    sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Publication {
    review_required: bool,
    checks: [&'static str; 4],
}

#[derive(Serialize)]
struct Manifest {
    schema: &'static str,
    source: FileHash,
    output: FileHash,
    render: Render,
    publication: Publication,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultAnswer<'a> {
    gif: &'a Path,
    manifest: &'a Path,
    source_sha256: &'a str,
    gif_sha256: &'a str,
    render: &'a Render,
    review_required: bool,
}

fn invalid(detail: impl Into<String>) -> Failure {
    Failure::invalid("readme-gif", detail)
}

fn bounded(value: f64, name: &str, maximum: f64) -> Result<f64, Failure> {
    if !value.is_finite() || value < 1.0 || value > maximum {
        return Err(invalid(format!("{name} must be between 1 and {maximum}")));
    }
    Ok(value)
}

fn non_negative(value: f64, name: &str) -> Result<f64, Failure> {
    if !value.is_finite() || value < 0.0 {
        return Err(invalid(format!("{name} must be a non-negative number")));
    }
    Ok(value)
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    std::path::absolute(path).map_err(|error| invalid(error.to_string()))
}

fn regular_file(file: &Path, label: &str) -> Result<PathBuf, Failure> {
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

fn output_file(file: &Path) -> Result<PathBuf, Failure> {
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

fn sha256(file: &Path) -> Result<String, Failure> {
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

fn json_number(value: f64) -> Value {
    if value.fract() == 0.0 && value <= u64::MAX as f64 {
        Value::Number((value as u64).into())
    } else {
        Value::Number(Number::from_f64(value).expect("validated finite number"))
    }
}

fn filter_graph(frames_per_second: f64, width: f64) -> String {
    format!(
        "[0:v]fps={frames_per_second},scale={width}:-2:flags=lanczos,split[v0][v1];[v0]palettegen=stats_mode=diff[p];[v1][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle[v]"
    )
}

pub fn create(options: Options) -> Answer {
    let input = regular_file(&options.input, "input video")?;
    let output = output_file(&options.output)?;
    if input == output {
        return Err(invalid("input video and --out must be different files"));
    }
    let sidecar = PathBuf::from(format!("{}.probierz.json", output.display()));
    if !options.force && (output.exists() || sidecar.exists()) {
        let existing = if output.exists() { &output } else { &sidecar };
        return Err(invalid(format!(
            "output already exists: {}; pass force=true to replace it",
            existing.display()
        )));
    }

    let start_seconds = non_negative(options.start_seconds, "startSeconds")?;
    let duration_seconds = bounded(
        options.duration_seconds,
        "durationSeconds",
        MAX_DURATION_SECONDS,
    )?;
    let frames_per_second = bounded(
        options.frames_per_second,
        "framesPerSecond",
        MAX_FRAMES_PER_SECOND,
    )?;
    let width = bounded(options.width, "width", MAX_WIDTH)?;
    if frames_per_second.fract() != 0.0 || width.fract() != 0.0 {
        return Err(invalid("framesPerSecond and width must be integers"));
    }
    let source_sha256 = sha256(&input)?;

    let output_directory = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_directory)?;
    let output_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output.gif");
    let temporary_stem = output_name.strip_suffix(".gif").unwrap_or(output_name);
    let temporary =
        output_directory.join(format!(".{temporary_stem}.{}.tmp.gif", std::process::id()));
    let temporary_sidecar =
        PathBuf::from(format!("{}.{}.tmp", sidecar.display(), std::process::id()));
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    if temporary_sidecar.exists() {
        fs::remove_file(&temporary_sidecar)?;
    }

    let ffmpeg = std::env::var("PROBIERZ_FFMPEG_BIN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "ffmpeg".to_string());
    let result = Command::new(ffmpeg.trim())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-ss"])
        .arg(start_seconds.to_string())
        .arg("-t")
        .arg(duration_seconds.to_string())
        .arg("-i")
        .arg(&input)
        .arg("-filter_complex")
        .arg(filter_graph(frames_per_second, width))
        .args(["-map", "[v]", "-loop", "0", "-y"])
        .arg(&temporary)
        .output();
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(invalid(format!("README GIF export failed: {error}")));
        }
    };
    if !result.status.success() {
        let _ = fs::remove_file(&temporary);
        let stderr = String::from_utf8_lossy(&result.stderr);
        let detail = if stderr.trim().is_empty() {
            format!(
                "exit {}",
                result
                    .status
                    .code()
                    .map_or_else(|| "null".to_string(), |code| code.to_string())
            )
        } else {
            stderr.trim().to_string()
        };
        return Err(invalid(format!("README GIF export failed: {detail}")));
    }

    if options.force && output.exists() {
        fs::remove_file(&output)?;
    }
    fs::rename(&temporary, &output)?;
    let gif_sha256 = sha256(&output)?;
    let render = Render {
        start_seconds: json_number(start_seconds),
        duration_seconds: json_number(duration_seconds),
        frames_per_second: json_number(frames_per_second),
        width: json_number(width),
        silent: true,
        r#loop: true,
    };
    let manifest = Manifest {
        schema: "probierz.readme-gif.v1",
        source: FileHash {
            file: input
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            sha256: source_sha256.clone(),
        },
        output: FileHash {
            file: output
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            sha256: gif_sha256.clone(),
        },
        render,
        publication: Publication {
            review_required: true,
            checks: [
                "one real end-to-end journey",
                "no credentials, personal data, production identifiers, or sensitive URLs",
                "readable at the rendered README size",
                "observable final outcome",
            ],
        },
    };
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    write_private(&temporary_sidecar, &manifest_bytes)?;
    if options.force && sidecar.exists() {
        fs::remove_file(&sidecar)?;
    }
    fs::rename(&temporary_sidecar, &sidecar)?;

    print_json(&ResultAnswer {
        gif: &output,
        manifest: &sidecar,
        source_sha256: &source_sha256,
        gif_sha256: &gif_sha256,
        render: &manifest.render,
        review_required: true,
    })
}
