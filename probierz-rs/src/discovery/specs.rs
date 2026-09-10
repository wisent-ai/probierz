use crate::discovery::*;
pub(crate) fn spec_files(harness: &Path, pkg: &str) -> Result<Vec<String>, Failure> {
    let mut found = Vec::new();
    for sub in SPEC_DIRS {
        let directory = harness.join(pkg).join(sub);
        if !directory.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&directory)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if SPEC_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
                found.push(format!("{pkg}/{sub}/{name}"));
            }
        }
    }
    found.sort();
    Ok(found)
}

#[derive(Debug, Serialize)]
pub(crate) struct Outline {
    pub(crate) spec: String,
    pub(crate) count: usize,
    pub(crate) outline: Vec<OutlineEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OutlineEntry {
    pub(crate) kind: String,
    pub(crate) title: String,
}

/// The describe / it / test titles of one spec, in file order. A pure text
/// scan: a title is what the file says, never what a run reported.
pub fn describe(harness: &Path, spec: &str) -> Answer {
    let clean = spec.trim_start_matches('/').to_string();
    let absolute = harness.join(&clean);
    let resolved = absolute
        .canonicalize()
        .map_err(|_| Failure::invalid("discovery.describe", format!("spec not found: {clean}")))?;
    let root = harness.canonicalize()?;
    if !resolved.starts_with(&root) {
        return Err(Failure::invalid(
            "discovery.describe",
            "path escapes the probierz root",
        ));
    }
    let source = std::fs::read_to_string(&resolved)?;
    let outline = outline_of(&source);
    print_json(&Outline {
        spec: clean,
        count: outline.len(),
        outline,
    })
}

/// Titles are read with a scanner rather than a regular expression so a
/// quotation mark inside a title cannot end it early.
pub(crate) fn outline_of(source: &str) -> Vec<OutlineEntry> {
    let mut entries = Vec::new();
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(kind) = ["describe", "it", "test"]
            .into_iter()
            .find(|kind| starts_call(source, bytes, index, kind))
        else {
            index += 1;
            continue;
        };
        let mut cursor = index + kind.len();
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        // `describe(` — anything else is an identifier that merely starts the
        // same way.
        if cursor >= bytes.len() || bytes[cursor] != b'(' {
            index += kind.len();
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || !matches!(bytes[cursor], b'\'' | b'"' | b'`') {
            index = cursor;
            continue;
        }
        let quote = bytes[cursor];
        cursor += 1;
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != quote {
            if bytes[cursor] == b'\\' {
                cursor += 1;
            }
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        entries.push(OutlineEntry {
            kind: kind.to_string(),
            title: source[start..cursor].to_string(),
        });
        index = cursor + 1;
    }
    entries
}

pub(crate) fn starts_call(source: &str, bytes: &[u8], index: usize, kind: &str) -> bool {
    if !source[index..].starts_with(kind) {
        return false;
    }
    let before_is_word = index > 0
        && (bytes[index - 1].is_ascii_alphanumeric()
            || matches!(bytes[index - 1], b'_' | b'$' | b'.'));
    !before_is_word
}
