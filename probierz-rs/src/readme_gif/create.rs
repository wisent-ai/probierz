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
    let duration_seconds = positive(options.duration_seconds, "durationSeconds")?;
    let frames_per_second = positive(options.frames_per_second, "framesPerSecond")?;
    let width = positive(options.width, "width")?;
    let whole = |value: f64| value.trunc() == value;
    if frames_per_second.is_some_and(|value| !whole(value))
        || width.is_some_and(|value| !whole(value))
    {
        return Err(invalid("framesPerSecond and width must be integers"));
    }
    let source_sha256 = crate::evidence::sha256_file(&input)?;

    // output_file resolved an absolute path ending in .gif, so it names a
    // file inside a directory; either missing is refused, never invented.
    let output_directory = output
        .parent()
        .ok_or_else(|| invalid(format!("--out {} names no directory", output.display())))?;
    fs::create_dir_all(output_directory)?;
    let temporary_stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| invalid(format!("--out {} has no file name", output.display())))?;
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

    let mut command = Command::new(ffmpeg()?);
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    if let Some(start_seconds) = start_seconds {
        command.arg("-ss").arg(start_seconds.to_string());
    }
    if let Some(duration_seconds) = duration_seconds {
        command.arg("-t").arg(duration_seconds.to_string());
    }
    // The GIF muxer loops forever unless told otherwise, which is the
    // looping README GIF this publishes.
    let result = stado_wait::output(
        command
            .arg("-i")
            .arg(&input)
            .arg("-filter_complex")
            .arg(filter_graph(frames_per_second, width))
            .args(["-map", "[v]", "-y"])
            .arg(&temporary),
    );
    // A failed export takes its partial GIF back; one that stays is named in
    // the refusal, never left unannounced beside the output.
    let refused = |reason: String| {
        invalid(format!("README GIF export failed: {reason}")).discarding(&temporary)
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => return Err(refused(error.to_string())),
    };
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let detail = if stderr.trim().is_empty() {
            match result.status.code() {
                Some(code) => format!("exit {code}"),
                None => format!("ended without an exit code: {}", result.status),
            }
        } else {
            stderr.trim().to_string()
        };
        return Err(refused(detail));
    }

    if options.force && output.exists() {
        fs::remove_file(&output)?;
    }
    fs::rename(&temporary, &output)?;
    let gif_sha256 = crate::evidence::sha256_file(&output)?;
    let render = Render {
        start_seconds,
        duration_seconds,
        frames_per_second,
        width,
        silent: true,
        r#loop: true,
    };
    let manifest = Manifest {
        schema: "probierz.readme-gif.v1",
        source: FileHash {
            file: file_name(&input)?,
            sha256: source_sha256.clone(),
        },
        output: FileHash {
            file: file_name(&output)?,
            sha256: gif_sha256.clone(),
        },
        render,
        publication: Publication {
            review_required: true,
            checks: vec![
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

/// The ffmpeg PROBIERZ_FFMPEG_BIN names, or the `ffmpeg` on PATH when it
/// names none; a value that is not Unicode is refused naming the variable.
fn ffmpeg() -> Result<String, Failure> {
    match std::env::var("PROBIERZ_FFMPEG_BIN") {
        Ok(value) if !value.trim().is_empty() => Ok(value.trim().to_string()),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok("ffmpeg".to_string()),
        Err(std::env::VarError::NotUnicode(raw)) => Err(invalid(format!(
            "PROBIERZ_FFMPEG_BIN is not valid Unicode: {raw:?}"
        ))),
    }
}

/// The file name a manifest records for `path`, refused when it has none.
fn file_name(path: &Path) -> Result<String, Failure> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| invalid(format!("{} has no file name", path.display())))
}
