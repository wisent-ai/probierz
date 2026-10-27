//! What a talk evaluation is given: the slide pictures in the order they are
//! shown (`slides`), the speaker script, the documents the talk must stay
//! true to (`source`), and the model router it asks (`router`).
//!
//! Every input is read once, here: the model is sent and the report digests
//! the same bytes, so a file changed during the run cannot be certified by a
//! digest of bytes the model never saw.

mod router;
mod slides;
mod source;

pub(crate) use router::*;
pub(crate) use source::Source;

use super::*;

/// The type a speaker script is written in.
const SCRIPT_EXTENSION: &str = "md";

/// One slide picture: the bytes read once, which the model is shown and the
/// report digests.
pub(crate) struct Slide {
    pub(crate) path: PathBuf,
    pub(crate) bytes: Vec<u8>,
}

/// The talk as it is judged.
pub(crate) struct Talk {
    pub(crate) slides: Vec<Slide>,
    pub(crate) script: PathBuf,
    pub(crate) script_bytes: Vec<u8>,
    pub(crate) script_text: String,
    pub(crate) sources: Vec<Source>,
}

impl Talk {
    pub(crate) fn resolve(
        slides: &Path,
        script: &Path,
        sources: &[PathBuf],
    ) -> Result<Self, Failure> {
        let slides = absolute(slides)?;
        let script = absolute(script)?;
        if sources.is_empty() {
            return Err(Failure::invalid(
                "evaluate.talk",
                "--source <document> is required: the talk's claims are judged against the documents it presents",
            ));
        }
        if script.extension().and_then(OsStr::to_str) != Some(SCRIPT_EXTENSION) {
            return Err(Failure::invalid(
                "evaluate.talk",
                format!(
                    "--script must be the speaker script Markdown (.md): {}",
                    script.display()
                ),
            ));
        }
        let script_bytes = read(&script, "--script")?;
        let script_text = utf8(&script_bytes, &script, "--script")?;
        let sources = sources
            .iter()
            .map(|path| source::source(&absolute(path)?))
            .collect::<Result<Vec<_>, Failure>>()?;
        let slides = slides::ordered_slides(&slides)?
            .into_iter()
            .map(|path| {
                let bytes = read(&path, "--slides")?;
                Ok(Slide { path, bytes })
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        Ok(Self {
            slides,
            script,
            script_bytes,
            script_text,
            sources,
        })
    }
}

/// The bytes of `path`, given as `flag`, refused naming both when unreadable.
fn read(path: &Path, flag: &str) -> Result<Vec<u8>, Failure> {
    fs::read(path).map_err(|error| {
        Failure::invalid(
            "evaluate.talk",
            format!("{flag} {}: {error}", path.display()),
        )
    })
}

/// `bytes` read from `path` as text, refused naming it when not UTF-8.
fn utf8(bytes: &[u8], path: &Path, flag: &str) -> Result<String, Failure> {
    String::from_utf8(bytes.to_vec()).map_err(|error| {
        Failure::invalid(
            "evaluate.talk",
            format!("{flag} {} is not UTF-8 text: {error}", path.display()),
        )
    })
}

/// Where the report goes: `--out`, which must be a JSON file that does not
/// exist yet, or a timestamped file under the harness's test results.
pub(crate) fn report_path(
    harness: &Path,
    talk: &Talk,
    output: Option<&Path>,
) -> Result<PathBuf, Failure> {
    let report = match output {
        Some(path) => absolute(path)?,
        None => {
            let stamp = chrono::Utc::now()
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                .replace([':', '.'], "-");
            let stem = talk
                .script
                .file_stem()
                .and_then(OsStr::to_str)
                .ok_or_else(|| {
                    Failure::invalid(
                        "evaluate.talk",
                        format!("{} has no file name", talk.script.display()),
                    )
                })?;
            harness
                .join("test-results/talk-evaluations")
                .join(format!("{stamp}-{stem}.probierz.json"))
        }
    };
    if report.extension().and_then(OsStr::to_str) != Some("json") {
        return Err(Failure::invalid("evaluate.talk", "--out must end in .json"));
    }
    if crate::paths::present(&report)? {
        return Err(Failure::invalid(
            "evaluate.talk",
            format!(
                "the talk evaluation report already exists: {}",
                report.display()
            ),
        ));
    }
    if let Some(directory) = report.parent() {
        fs::create_dir_all(directory).map_err(|error| {
            Failure::invalid(
                "evaluate.talk",
                format!("the report directory {}: {error}", directory.display()),
            )
        })?;
    }
    Ok(report)
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let directory = std::env::current_dir().map_err(|error| {
        Failure::invalid(
            "evaluate.talk",
            format!(
                "{} is relative and the working directory cannot be read: {error}",
                path.display()
            ),
        )
    })?;
    Ok(directory.join(path))
}
