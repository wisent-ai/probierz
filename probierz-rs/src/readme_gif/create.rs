use crate::readme_gif::*;
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
