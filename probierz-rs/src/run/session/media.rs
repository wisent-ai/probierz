use crate::run::*;
use serde_json::json;
/// Every file under `root`. A directory or entry that cannot be read is left
/// out and named on stderr with its cause: left out in silence, a run's
/// evidence lost files with nothing saying they were there.
pub(crate) fn walk(root: &Path, sort: bool) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let listing = match fs::read_dir(root) {
        Ok(listing) => listing,
        Err(error) => {
            eprintln!("probierz: {} cannot be listed: {error}", root.display());
            return Vec::new();
        }
    };
    let mut entries = Vec::new();
    for entry in listing {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => eprintln!(
                "probierz: an entry of {} cannot be read: {error}",
                root.display()
            ),
        }
    }
    if sort {
        entries.sort_by_key(|entry| entry.file_name());
    }
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path, sort));
        } else {
            files.push(path);
        }
    }
    files
}

/// The size in kilobytes, rounded, or `None` when it cannot be read: a report
/// then carries `null` and the cause is named on stderr, where it used to say
/// 0 KB and look like an empty recording.
pub(crate) fn size_kb(path: &Path) -> Option<u64> {
    match fs::metadata(path) {
        Ok(meta) => Some((meta.len() + 512) / 1024),
        Err(error) => {
            eprintln!(
                "probierz: the size of {} cannot be read: {error}",
                path.display()
            );
            None
        }
    }
}

fn media_output(program: &str, args: &[String], file: &Path) -> Result<Vec<u8>, String> {
    let result = capture(program, args, None, None);
    if let Some(error) = result.error {
        return Err(format!(
            "{program} {}: {error}; stderr: {}",
            file.display(),
            text(&result.stderr)
        ));
    }
    match result.status {
        Some(status) if status.success() => Ok(result.stdout),
        Some(status) => Err(format!(
            "{program} {}: {status}; stderr: {}",
            file.display(),
            text(&result.stderr)
        )),
        None => Err(format!(
            "{program} {}: no exit status; stderr: {}",
            file.display(),
            text(&result.stderr)
        )),
    }
}

pub(crate) fn probe_video(file: &Path) -> Result<Value, String> {
    let output = media_output(
        "ffprobe",
        &[
            "-v".into(),
            "error".into(),
            "-select_streams".into(),
            "v:0".into(),
            "-show_entries".into(),
            "stream=width,height:format=duration".into(),
            "-of".into(),
            "json".into(),
            file.to_string_lossy().into_owned(),
        ],
        file,
    )?;
    let document: Value = serde_json::from_slice(&output)
        .map_err(|error| format!("decode ffprobe output for {}: {error}", file.display()))?;
    let stream = document
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| streams.first())
        .ok_or_else(|| format!("ffprobe {} returned no video stream", file.display()))?;
    let duration = match document.pointer("/format/duration") {
        None => Value::Null,
        Some(value) => {
            let duration = value
                .as_str()
                .ok_or_else(|| {
                    format!(
                        "ffprobe {} returned a non-text duration: {value}",
                        file.display()
                    )
                })?
                .parse::<f64>()
                .map_err(|error| format!("decode ffprobe duration for {}: {error}", file.display()))?;
            if !duration.is_finite() {
                return Err(format!(
                    "ffprobe {} returned non-finite duration: {value}",
                    file.display()
                ));
            }
            number(duration)
        }
    };
    Ok(json!({ "durationSec": duration, "width": stream.get("width"), "height": stream.get("height") }))
}

pub(crate) fn extract_frames(
    video: &Path,
    artifacts: &Path,
    count: f64,
    metadata: &Value,
) -> Result<Vec<PathBuf>, String> {
    let stem = video
        .file_stem()
        .ok_or_else(|| format!("recording {} has no file stem", video.display()))?;
    let output = artifacts.join("frames").join(stem);
    let duration = metadata
        .get("durationSec")
        .and_then(Value::as_f64)
        .filter(|duration| *duration > 0.0)
        .ok_or_else(|| {
            format!(
                "ffprobe {} did not report a positive recording duration",
                video.display()
            )
        })?;
    let n = count.max(1.0);
    let fps = n / duration;
    match fs::remove_dir_all(&output) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "remove frame directory {}: {error}",
                output.display()
            ));
        }
    }
    fs::create_dir_all(&output)
        .map_err(|error| format!("create frame directory {}: {error}", output.display()))?;
    let pattern = output.join("frame_%03d.png");
    media_output(
        "ffmpeg",
        &[
            "-y".into(),
            "-i".into(),
            video.to_string_lossy().into_owned(),
            "-vf".into(),
            format!("fps={fps}"),
            pattern.to_string_lossy().into_owned(),
        ],
        video,
    )?;
    let mut frames = fs::read_dir(&output)
        .map_err(|error| format!("list extracted frames in {}: {error}", output.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read extracted frame entry in {}: {error}", output.display()))?;
    frames.sort();
    if frames.is_empty() {
        return Err(format!(
            "ffmpeg {} exited successfully but produced no frames in {}",
            video.display(),
            output.display()
        ));
    }
    Ok(frames)
}

pub(crate) fn number(value: f64) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }
    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        return Value::Number(Number::from(value as i64));
    }
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}
pub(crate) fn js_number(value: Option<&Value>) -> f64 {
    value
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(0.0)
}
