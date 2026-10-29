//! What the evaluation is given: the two figures, the model router and
//! the credentials for it, and where the report and its renders go.
//!
//! An output path that already exists is refused rather than
//! overwritten: a figure evaluation is evidence, and the renders beside
//! it are part of it. Which figure formats can be evaluated is the
//! renderer's to say: a file it cannot read fails its render with the
//! renderer's own error, so no second list of formats is kept here.

use super::*;

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
            let metadata = fs::metadata(file).map_err(|error| {
                Failure::invalid(
                    "figure-evaluate",
                    format!("{label} {} cannot be read: {error}", file.display()),
                )
            })?;
            if !metadata.is_file() {
                return Err(Failure::invalid(
                    "figure-evaluate",
                    format!("{label} is not a file: {}", file.display()),
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

        let token = setting(router_bearer, "STADO_MODEL_ROUTER_TOKEN")?.ok_or_else(|| {
            Failure::config(
                "figure-evaluate",
                "STADO_MODEL_ROUTER_TOKEN or an explicit router bearer is required",
            )
        })?;
        if token.chars().any(char::is_whitespace) {
            return Err(Failure::invalid(
                "figure-evaluate",
                "model router bearer must not contain whitespace",
            ));
        }

        let (agent_id, agent_secret) = match (
            setting(agent_id, "PROBIERZ_MODEL_AGENT_ID")?,
            setting(agent_secret, "PROBIERZ_MODEL_AGENT_SECRET")?,
        ) {
            (Some(id), Some(secret)) => (id, secret),
            (None, None) => (String::new(), String::new()),
            _ => {
                return Err(Failure::config(
                    "figure-evaluate",
                    "agent identity needs both an agent ID and an agent secret",
                ))
            }
        };

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
            None => default_report_path(harness, &pair.candidate)?,
        };
        let is_json = report
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
        if !is_json {
            return Err(Failure::invalid(
                "figure-evaluate",
                "figure evaluation --out must end in .json",
            ));
        }
        let directory = report.parent().ok_or_else(|| {
            Failure::invalid(
                "figure-evaluate",
                format!("figure evaluation --out {} names no directory", report.display()),
            )
        })?;
        let stem = report.file_stem().and_then(OsStr::to_str).ok_or_else(|| {
            Failure::invalid(
                "figure-evaluate",
                format!("figure evaluation --out {} names no text file name", report.display()),
            )
        })?;
        let destination = Self {
            reference_output: directory.join(format!("{stem}-reference.png")),
            candidate_output: directory.join(format!("{stem}-candidate.png")),
            report: report.clone(),
        };
        for file in [
            &destination.report,
            &destination.reference_output,
            &destination.candidate_output,
        ] {
            match fs::symlink_metadata(file) {
                Ok(_) => {
                    return Err(Failure::invalid(
                        "figure-evaluate",
                        format!("figure evaluation output already exists: {}", file.display()),
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(Failure::invalid(
                        "figure-evaluate",
                        format!(
                            "figure evaluation output {} cannot be examined: {error}",
                            file.display()
                        ),
                    ))
                }
            }
        }
        fs::create_dir_all(directory)?;
        Ok(destination)
    }
}

/// A private working directory for the renders, beside the report it
/// belongs to and named after it and this process, so two runs never share
/// one and nothing is written outside the place the caller chose.
pub(crate) fn work_directory(report: &Path) -> Result<PathBuf, Failure> {
    let name = report.file_name().ok_or_else(|| {
        Failure::invalid(
            "figure-evaluate",
            format!("figure evaluation report {} names no file", report.display()),
        )
    })?;
    let mut work_name = name.to_os_string();
    work_name.push(format!(".renders-{}", std::process::id()));
    let work = report.with_file_name(work_name);
    fs::create_dir(&work)?;
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

/// A setting the caller stated, else the environment's; None when neither
/// states one, and a refusal naming the variable when it is set but not text.
fn setting(explicit: Option<&str>, name: &str) -> Result<Option<String>, Failure> {
    if let Some(value) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(Some(value.to_string()));
    }
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(Failure::config(
            "figure-evaluate",
            format!("{name} is set but is not text; set it to the value, or unset it"),
        )),
    }
}

/// Where a report goes when the caller names no path: a timestamped
/// file under the current directory's results, named after the
/// candidate.
fn default_report_path(harness: &Path, candidate: &Path) -> Result<PathBuf, Failure> {
    let stem = candidate.file_stem().and_then(OsStr::to_str).ok_or_else(|| {
        Failure::invalid(
            "figure-evaluate",
            format!(
                "candidate {} names no text file name to title its report",
                candidate.display()
            ),
        )
    })?;
    let stem = stem
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
    let base = std::env::current_dir().map_err(|error| {
        Failure::config(
            "figure-evaluate",
            format!(
                "the current directory cannot be read to place the report ({error}); name one with --out, for example under {}",
                harness.display()
            ),
        )
    })?;
    Ok(base
        .join("test-results/figure-evaluations")
        .join(format!("{stamp}-{stem}.probierz.json")))
}
