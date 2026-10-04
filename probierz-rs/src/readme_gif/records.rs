use crate::readme_gif::*;

#[derive(Debug, Clone)]
pub struct Options {
    pub input: PathBuf,
    pub output: PathBuf,
    pub start_seconds: f64,
    /// Seconds kept from `start_seconds`; the rest of the clip when `None`.
    pub duration_seconds: Option<f64>,
    /// Frame rate; the source's own when `None`.
    pub frames_per_second: Option<f64>,
    /// Width in pixels; the source's own when `None`.
    pub width: Option<f64>,
    pub force: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Render {
    pub(crate) start_seconds: Value,
    pub(crate) duration_seconds: Value,
    pub(crate) frames_per_second: Value,
    pub(crate) width: Value,
    pub(crate) silent: bool,
    pub(crate) r#loop: bool,
}

#[derive(Serialize)]
pub(crate) struct FileHash {
    pub(crate) file: String,
    pub(crate) sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Publication {
    pub(crate) review_required: bool,
    pub(crate) checks: [&'static str; 4],
}

#[derive(Serialize)]
pub(crate) struct Manifest {
    pub(crate) schema: &'static str,
    pub(crate) source: FileHash,
    pub(crate) output: FileHash,
    pub(crate) render: Render,
    pub(crate) publication: Publication,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultAnswer<'a> {
    pub(crate) gif: &'a Path,
    pub(crate) manifest: &'a Path,
    pub(crate) source_sha256: &'a str,
    pub(crate) gif_sha256: &'a str,
    pub(crate) render: &'a Render,
    pub(crate) review_required: bool,
}
