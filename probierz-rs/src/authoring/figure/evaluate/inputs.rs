//! What the evaluation is given: the two figures, the model router and
//! the credentials for it, and where the report and its renders go.
//!
//! An output path that already exists is refused rather than
//! overwritten: a figure evaluation is evidence, and the renders beside
//! it are part of it.

use super::*;

/// Figure formats this evaluation can render.
const SUPPORTED: [&str; 7] = ["jpeg", "jpg", "pdf", "png", "svg", "tex", "webp"];

/// The two figures, as absolute paths.
pub(crate) struct FigurePair {
    pub(crate) reference: PathBuf,
    pub(crate) candidate: PathBuf,
}

impl FigurePair {
    pub(crate) fn resolve(reference: &Path, candidate: &Path) -> Result<Self, Failure> {
        let pair = Self {
            reference: absolute(reference)?,
            candidate: absolute(candidate)?,
        };
        for (label, file) in [
            ("reference", &pair.reference),
            ("candidate", &pair.candidate),
        ] {
            if !file.is_file() {
                return Err(Failure::invalid(
                    "figure-evaluate",
                    format!("{label} is not a file: {}", file.display()),
                ));
            }
            let extension = extension_of(file);
            if !SUPPORTED.contains(&extension.as_str()) {
                return Err(Failure::invalid(
                    "figure-evaluate",
                    format!(
                        "{label} type is not supported: {}",
                        if extension.is_empty() {
                            "no extension"
                        } else {
                            &extension
                        }
                    ),
                ));
            }
        }
        Ok(pair)
    }

    /// Whether either figure is LaTeX, which is what makes pdflatex a
    /// recorded part of the renderer identity.
    pub(crate) fn uses_latex(&self) -> bool {
        self.reference.extension() == Some(OsStr::new("tex"))
            || self.candidate.extension() == Some(OsStr::new("tex"))
    }
}

/// The model router this evaluation asks, and the identity it asks
/// with.
pub(crate) struct Router {
    pub(crate) model: String,
    pub(crate) url: String,
    pub(crate) token: String,
    pub(crate) agent_id: String,
    pub(crate) agent_secret: String,
}

impl Router {
    pub(crate) fn resolve(
        harness: &Path,
        model: Option<&str>,
        router_url: Option<&str>,
        router_bearer: Option<&str>,
        agent_id: Option<&str>,
        agent_secret: Option<&str>,
    ) -> Result<Self, Failure> {
        let (model, url) = figure_prerequisites(harness, None, None, model, router_url)
            .map_err(|detail| Failure::config("figure-evaluate", detail))?;

        let token = setting(router_bearer, "STADO_MODEL_ROUTER_TOKEN");
        if token.is_empty() {
            return Err(Failure::config(
                "figure-evaluate",
                "STADO_MODEL_ROUTER_TOKEN or an explicit router bearer is required",
            ));
        }
        if token.chars().any(char::is_whitespace) {
            return Err(Failure::invalid(
                "figure-evaluate",
                "model router bearer must not contain whitespace",
            ));
        }

        let agent_id = setting(agent_id, "PROBIERZ_MODEL_AGENT_ID");
        let agent_secret = setting(agent_secret, "PROBIERZ_MODEL_AGENT_SECRET");
        if agent_id.is_empty() != agent_secret.is_empty() {
            return Err(Failure::config(
                "figure-evaluate",
                "agent identity needs both an agent ID and an agent secret",
            ));
        }

        Ok(Self {
            model,
            url,
            token,
            agent_id,
            agent_secret,
        })
    }
}

/// Where this evaluation writes: the report, and the two PNG renders
/// beside it.
pub(crate) struct Destination {
    pub(crate) report: PathBuf,
    pub(crate) reference_output: PathBuf,
    pub(crate) candidate_output: PathBuf,
}

impl Destination {
    pub(crate) fn resolve(
        harness: &Path,
        pair: &FigurePair,
        output: Option<&Path>,
    ) -> Result<Self, Failure> {
        let report = match output {
            Some(path) => absolute(path)?,
            None => default_report_path(harness, &pair.candidate),
        };
        if report
            .extension()
            .and_then(OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some("json")
        {
            return Err(Failure::invalid(
                "figure-evaluate",
                "figure evaluation --out must end in .json",
            ));
        }
        let directory = report
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let stem = report
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or("figure")
            .to_string();
        let destination = Self {
            reference_output: directory.join(format!("{stem}-reference.png")),
            candidate_output: directory.join(format!("{stem}-candidate.png")),
            report,
        };
        for file in [
            &destination.report,
            &destination.reference_output,
            &destination.candidate_output,
        ] {
            if file.exists() {
                return Err(Failure::invalid(
                    "figure-evaluate",
                    format!(
                        "figure evaluation output already exists: {}",
                        file.display()
                    ),
                ));
            }
        }
        fs::create_dir_all(directory)?;
        Ok(destination)
    }
}

/// A private working directory for the renders, named after this
/// process and the moment it started so two runs never share one.
pub(crate) fn work_directory() -> Result<PathBuf, Failure> {
    let work = std::env::temp_dir().join(format!(
        "probierz-figure-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Failure::config("figure-evaluate", error.to_string()))?
            .as_nanos()
    ));
    fs::create_dir_all(&work)?;
    Ok(work)
}

/// Render one figure, reporting a renderer failure as a render error.
pub(crate) fn render(
    file: &Path,
    work: &Path,
    label: &str,
    tex_preamble: Option<&Path>,
) -> Result<PathBuf, Failure> {
    render_figure(file, work, label, tex_preamble)
        .map_err(|detail| Failure::config("figure-evaluate.render", detail))
}

/// Measure one render's geometry.
pub(crate) fn geometry(render: &Path) -> Result<JsonValue, Failure> {
    figure_geometry(render).map_err(|detail| Failure::config("figure-evaluate.render", detail))
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn extension_of(file: &Path) -> String {
    file.extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn setting(explicit: Option<&str>, name: &str) -> String {
    explicit
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var(name).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Where a report goes when the caller names no path: a timestamped
/// file under the current directory's results, named after the
/// candidate.
fn default_report_path(harness: &Path, candidate: &Path) -> PathBuf {
    let stem = candidate
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("figure")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let stamp = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .replace([':', '.'], "-");
    std::env::current_dir()
        .unwrap_or_else(|_| harness.to_path_buf())
        .join("test-results/figure-evaluations")
        .join(format!("{stamp}-{stem}.probierz.json"))
}
