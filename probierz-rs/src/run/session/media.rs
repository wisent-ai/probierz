use serde_json::json;
use crate::run::*;
pub(crate) fn walk(root: &Path, sort: bool) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let mut entries: Vec<_> = fs::read_dir(root).into_iter().flatten().flatten().collect();
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

pub(crate) fn size_kb(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|meta| (meta.len() + 512) / 1024)
        .unwrap_or(0)
}

pub(crate) fn has_media_binary(name: &str) -> bool {
    successful(name, &["-version"])
}

pub(crate) fn probe_video(file: &Path) -> Option<Value> {
    if !has_media_binary("ffprobe") {
        return None;
    }
    let result = capture(
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
        None,
        None,
        None,
    );
    if !result.status.is_some_and(|status| status.success()) || result.stdout.is_empty() {
        return None;
    }
    let document: Value = serde_json::from_slice(&result.stdout).ok()?;
    let stream = document
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| streams.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let duration = document
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .map(number)
        .unwrap_or(Value::Null);
    Some(
        json!({ "durationSec": duration, "width": stream.get("width").cloned().unwrap_or(Value::Null), "height": stream.get("height").cloned().unwrap_or(Value::Null) }),
    )
}

pub(crate) fn extract_frames(video: &Path, artifacts: &Path, count: f64) -> Vec<PathBuf> {
    let stem = video
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let output = artifacts.join("frames").join(stem);
    let _ = fs::remove_dir_all(&output);
    if !has_media_binary("ffmpeg") {
        return Vec::new();
    }
    let _ = fs::create_dir_all(&output);
    let duration = probe_video(video)
        .and_then(|meta| meta.get("durationSec").and_then(Value::as_f64))
        .unwrap_or(0.0);
    let n = count.max(1.0);
    let fps = if duration > 0.0 { n / duration } else { 1.0 };
    let pattern = output.join("frame_%03d.png");
    let result = capture(
        "ffmpeg",
        &[
            "-y".into(),
            "-i".into(),
            video.to_string_lossy().into_owned(),
            "-vf".into(),
            format!("fps={fps}"),
            pattern.to_string_lossy().into_owned(),
        ],
        None,
        None,
        None,
    );
    if !result.status.is_some_and(|status| status.success()) {
        return Vec::new();
    }
    walk(&output, true)
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

