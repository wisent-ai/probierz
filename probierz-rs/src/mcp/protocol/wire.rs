use crate::*;
pub(crate) fn send(value: &Value) {
    let mut stdout = io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, value);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

pub(crate) fn harness_root() -> PathBuf {
    if let Some(root) = std::env::var_os("PROBIERZ_HARNESS") {
        return PathBuf::from(root);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")))
        .to_path_buf()
}

pub(crate) fn probierz_binary() -> PathBuf {
    if let Some(binary) = std::env::var_os("PROBIERZ_BIN") {
        return PathBuf::from(binary);
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("probierz")))
        .unwrap_or_else(|| PathBuf::from("probierz"))
}

pub(crate) fn non_empty<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str, String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{name} must be a non-empty string"))
}

pub(crate) fn kebab(name: &str) -> String {
    let mut result = String::new();
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            result.push('-');
            result.push(character.to_ascii_lowercase());
        } else {
            result.push(character);
        }
    }
    result
}

pub(crate) fn append_flag(arguments: &mut Vec<String>, name: &str, value: &Value) {
    let flag = format!("--{}", kebab(name));
    match value {
        Value::Bool(true) => arguments.push(flag),
        Value::Bool(false) | Value::Null => {}
        Value::String(text) => {
            arguments.push(flag);
            arguments.push(text.clone());
        }
        Value::Number(number) => {
            arguments.push(flag);
            arguments.push(number.to_string());
        }
        Value::Array(items) => {
            for item in items {
                append_flag(arguments, name, item);
            }
        }
        Value::Object(_) => {}
    }
}

