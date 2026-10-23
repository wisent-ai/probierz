//! The documents a talk presents. Each is read once: the report digests
//! those bytes and the model reads the text taken from them, Markdown, plain
//! text and TeX as written, a PDF as `pdftotext -layout` extracts it from the
//! same bytes on its standard input.

use std::io::Write;
use std::process::Stdio;

use super::*;

/// One document the talk presents: the bytes read once, which the report
/// digests, and the full text the model reads, taken from those same bytes.
pub(crate) struct Source {
    pub(crate) path: PathBuf,
    pub(crate) bytes: Vec<u8>,
    pub(crate) text: String,
}

/// A source document read once. Any type other than Markdown, text, TeX or
/// PDF, and a document with no text, is refused naming it.
pub(super) fn source(path: &Path) -> Result<Source, Failure> {
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);
    let portable = match extension.as_deref() {
        Some("md" | "txt" | "tex") => false,
        Some("pdf") => true,
        _ => {
            return Err(Failure::invalid(
                "evaluate.talk",
                format!(
                    "--source {} must be Markdown, text, TeX or PDF",
                    path.display()
                ),
            ))
        }
    };
    let bytes = read(path, "--source")?;
    let text = match portable {
        true => pdf_text(path, &bytes)?,
        false => utf8(&bytes, path, "--source")?,
    };
    if text.trim().is_empty() {
        return Err(Failure::invalid(
            "evaluate.talk",
            format!(
                "--source {} holds no text to judge the talk against",
                path.display()
            ),
        ));
    }
    Ok(Source {
        path: path.to_path_buf(),
        bytes,
        text,
    })
}

/// The text of the PDF `bytes` read from `path`, as `pdftotext -layout`
/// extracts it from those bytes on its standard input.
fn pdf_text(path: &Path, bytes: &[u8]) -> Result<String, Failure> {
    let mut child = Command::new("pdftotext")
        .args(["-layout", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            Failure::config(
                "evaluate.talk",
                format!("pdftotext (poppler) reads a PDF source and could not start: {error}"),
            )
        })?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        Failure::config(
            "evaluate.talk",
            "pdftotext was started without the standard input the PDF is written to",
        )
    })?;
    // Written from its own thread, so pdftotext writing its text while the
    // PDF is still being written cannot fill a pipe both sides wait on.
    let pdf = bytes.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&pdf));
    let output = stado_wait::child_output(child, format!("pdftotext of {}", path.display()))
        .map_err(|error| {
            Failure::config(
                "evaluate.talk",
                format!(
                    "pdftotext of --source {} could not be read: {error}",
                    path.display()
                ),
            )
        })?;
    let written = writer.join().map_err(|_| {
        Failure::config(
            "evaluate.talk",
            format!("writing --source {} to pdftotext panicked", path.display()),
        )
    })?;
    if !output.status.success() {
        return Err(Failure::invalid(
            "evaluate.talk",
            format!(
                "pdftotext could not read --source {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    written.map_err(|error| {
        Failure::config(
            "evaluate.talk",
            format!(
                "--source {} could not be written to pdftotext: {error}",
                path.display()
            ),
        )
    })?;
    utf8(&output.stdout, path, "--source")
}
