use crate::mcp::*;
pub(crate) fn send(value: &Value) {
    let mut stdout = io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, value);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

/// This binary, for the one tool that runs as a child process: an
/// asynchronous run, whose process tree is cancelled as a whole.
/// The executable serving this MCP process, so a job runs the same build.
/// One that cannot be found is the refusal naming why, never a `probierz`
/// looked up on PATH, which could be another build.
pub(crate) fn probierz_binary() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|error| format!("cannot find this probierz executable to run the job: {error}"))
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
