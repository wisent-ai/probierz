use serde_json::json;
use crate::authoring::*;
pub(crate) fn files_below(root: &Path, extension: &str) -> Result<Vec<PathBuf>, Failure> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                let name = entry.file_name();
                if !matches!(
                    name.to_str(),
                    Some(".build" | ".git" | ".swiftpm" | "dist" | "node_modules" | "test-results")
                ) {
                    pending.push(entry.path());
                }
            } else if file_type.is_file()
                && entry.file_name().to_string_lossy().ends_with(extension)
            {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

pub(crate) fn line_number(content: &str, offset: usize) -> usize {
    1 + content.as_bytes()[..offset.min(content.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

pub(crate) fn literal_at(content: &str, start: usize) -> Option<(String, usize)> {
    let quote = *content.as_bytes().get(start)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let bytes = content.as_bytes();
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == quote && (index == start + 1 || bytes[index - 1] != b'\\') {
            return Some((content[start + 1..index].to_string(), index + 1));
        }
        index += 1;
    }
    None
}

pub(crate) fn valid_identifier(value: &str, require_dot: bool) -> bool {
    let groups: Vec<&str> = value.split('.').collect();
    if require_dot && groups.len() < 2 {
        return false;
    }
    groups.iter().enumerate().all(|(index, group)| {
        !group.is_empty()
            && group.bytes().enumerate().all(|(position, byte)| {
                if index == 0 && position == 0 {
                    byte.is_ascii_lowercase()
                } else if index == 0 {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                } else {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
                }
            })
    })
}

pub(crate) fn explicit_accessibility_identifiers(
    content: &str,
    file: &Path,
    repository: &Path,
) -> Vec<JsonValue> {
    let needle = ".accessibilityIdentifier";
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let start = cursor + relative;
        let mut index = start + needle.len();
        while content
            .as_bytes()
            .get(index)
            .is_some_and(u8::is_ascii_whitespace)
        {
            index += 1;
        }
        if content.as_bytes().get(index) != Some(&b'(') {
            cursor = index;
            continue;
        }
        index += 1;
        while content
            .as_bytes()
            .get(index)
            .is_some_and(u8::is_ascii_whitespace)
        {
            index += 1;
        }
        if let Some((value, end)) = literal_at(content, index) {
            let mut close = end;
            while content
                .as_bytes()
                .get(close)
                .is_some_and(u8::is_ascii_whitespace)
            {
                close += 1;
            }
            if content.as_bytes().get(close) == Some(&b')') {
                found.push(json!({ "value": value, "file": file.to_string_lossy(), "line": line_number(content, start), "repository": repository.to_string_lossy() }));
            }
        }
        cursor = index.saturating_add(1);
    }
    found
}

pub(crate) fn swift_dynamic_prefixes(content: &str) -> BTreeSet<String> {
    let mut prefixes = BTreeSet::new();
    for (index, byte) in content.bytes().enumerate() {
        if byte != b'\'' && byte != b'"' {
            continue;
        }
        let rest = &content[index + 1..];
        if let Some(interpolation) = rest.find("\\(") {
            let prefix = &rest[..interpolation];
            if prefix.ends_with('.') && valid_identifier(prefix.trim_end_matches('.'), false) {
                prefixes.insert(prefix.to_string());
            }
        }
    }
    prefixes
}

pub(crate) fn quoted_values(content: &str) -> Vec<(String, usize)> {
    let mut values = Vec::new();
    let mut index = 0;
    while index < content.len() {
        if let Some((value, end)) = literal_at(content, index) {
            values.push((value, index));
            index = end;
        } else {
            index += content[index..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1);
        }
    }
    values
}

pub(crate) fn yaml_string<'a>(value: &'a YamlValue, key: &str) -> Option<&'a str> {
    value.get(key).and_then(YamlValue::as_str)
}

