use crate::readme_gif::*;
pub(crate) const MAX_DURATION_SECONDS: f64 = 30.0;
pub(crate) const MAX_FRAMES_PER_SECOND: f64 = 20.0;
pub(crate) const MAX_WIDTH: f64 = 1200.0;

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

