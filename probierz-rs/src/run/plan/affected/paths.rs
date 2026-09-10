use crate::run::*;
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

pub(crate) fn slash(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

pub(crate) fn glob_matches(pattern: &str, value: &str) -> bool {
    fn go(
        p: &[u8],
        v: &[u8],
        memo: &mut HashMap<(usize, usize), bool>,
        pi: usize,
        vi: usize,
    ) -> bool {
        if let Some(answer) = memo.get(&(pi, vi)) {
            return *answer;
        }
        let answer = if pi == p.len() {
            vi == v.len()
        } else if p[pi] == b'*' && pi + 1 < p.len() && p[pi + 1] == b'*' {
            go(p, v, memo, pi + 2, vi) || (vi < v.len() && go(p, v, memo, pi, vi + 1))
        } else if p[pi] == b'*' {
            go(p, v, memo, pi + 1, vi)
                || (vi < v.len() && v[vi] != b'/' && go(p, v, memo, pi, vi + 1))
        } else {
            vi < v.len() && p[pi] == v[vi] && go(p, v, memo, pi + 1, vi + 1)
        };
        memo.insert((pi, vi), answer);
        answer
    }
    go(
        pattern.as_bytes(),
        value.as_bytes(),
        &mut HashMap::new(),
        0,
        0,
    )
}

pub(crate) fn yaml_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(value) => Some(value.clone()),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

