use crate::specs::*;

/// One journey: a name an operator reads in the report, and the function that
/// performs it. A journey answers with the reason it failed, never a panic —
/// the runner still catches those, because a panic in one journey must not
/// take the rest of the surface with it.
pub struct Spec {
    pub surface: &'static str,
    pub title: &'static str,
    pub run: fn(&Context) -> Result<(), String>,
}

/// What a journey is given: where artifacts go, and the environment the
/// operator provisioned. Nothing is invented here — a journey that needs a
/// binary, a model, or an account says which variable is missing and stops.
pub struct Context {
    pub harness: PathBuf,
    pub artifacts: PathBuf,
    pub title: String,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) media: Mutex<Vec<Media>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Media {
    pub file: PathBuf,
    pub kind: &'static str,
    #[serde(rename = "contentType", skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

impl Context {
    /// A variable the operator must have provisioned. The message names the
    /// variable and what it has to point at, because a journey cannot create
    /// a real account, a real subscription, or a released binary.
    pub fn required(&self, name: &str, what: &str) -> Result<String, String> {
        match self.env.get(name).map(|value| value.trim().to_string()) {
            Some(value) if !value.is_empty() => Ok(value),
            _ => Err(format!(
                "{name} is required: {what}; Probierz never invents or provisions provider access"
            )),
        }
    }

    /// A variable a journey may use when it is set.
    pub fn optional(&self, name: &str) -> Option<String> {
        self.env
            .get(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    /// Record a screenshot, trace, or video this journey produced. The runner
    /// refuses paths outside the artifacts directory and files that are not
    /// there, so a report never points at evidence that does not exist.
    pub fn media(&self, kind: &'static str, file: impl Into<PathBuf>) {
        let entry = Media {
            file: file.into(),
            kind,
            content_type: None,
        };
        self.media.lock().expect("media lock").push(entry);
    }

    pub fn media_typed(&self, kind: &'static str, file: impl Into<PathBuf>, content_type: &str) {
        let entry = Media {
            file: file.into(),
            kind,
            content_type: Some(content_type.to_string()),
        };
        self.media.lock().expect("media lock").push(entry);
    }

    pub(crate) fn declared_media(&self) -> Vec<Media> {
        self.media.lock().expect("media lock").clone()
    }
}

/// Every journey this toolkit owns, in the order a report lists them.
pub fn registry() -> Vec<Spec> {
    let mut all = Vec::new();
    all.extend(tui::specs());
    all.extend(cua::specs());
    all.sort_by(|left, right| (left.surface, left.title).cmp(&(right.surface, right.title)));
    all
}

/// The journeys of one surface, optionally narrowed to a title or a prefix.
pub fn select(surface: &str, filter: Option<&str>) -> Vec<Spec> {
    registry()
        .into_iter()
        .filter(|spec| spec.surface == surface)
        .filter(|spec| match filter {
            None => true,
            Some(want) => match want.strip_suffix('*') {
                Some(prefix) => spec.title.starts_with(prefix),
                None => spec.title == want,
            },
        })
        .collect()
}

pub(crate) fn at_iso(base: SystemTime, elapsed: Duration) -> String {
    iso_timestamp(base + elapsed)
}

/// An operator reads these lines on a terminal: keep the headline that says
/// what was expected and the tail that says what the application was showing.
/// A long screen dump never drowns the reason.
pub(crate) fn clip_row_error(text: &str) -> String {
    const LIMIT: usize = 2000;
    const HEAD: usize = 600;
    const TAIL: usize = 1400;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let head: String = chars[..HEAD].iter().collect();
    let tail: String = chars[chars.len() - TAIL..].iter().collect();
    format!("{head}\n...\n{tail}")
}

pub(crate) fn validate_media(artifacts: &Path, declared: &[Media]) -> Result<Vec<Media>, String> {
    let root = fs::canonicalize(artifacts).unwrap_or_else(|_| artifacts.to_path_buf());
    let mut out = Vec::with_capacity(declared.len());
    for entry in declared {
        if !["screenshot", "trace", "video"].contains(&entry.kind) {
            return Err(format!("unsupported media kind {}", entry.kind));
        }
        let resolved = fs::canonicalize(&entry.file)
            .map_err(|_| format!("declared media does not exist: {}", entry.file.display()))?;
        if resolved != root && !resolved.starts_with(&root) {
            return Err("media path escapes the artifacts directory".to_string());
        }
        out.push(Media {
            file: resolved,
            kind: entry.kind,
            content_type: entry.content_type.clone(),
        });
    }
    Ok(out)
}

