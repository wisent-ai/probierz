use crate::cua::*;
use serde_json::json;
impl Snapshot {
    pub fn from_value(raw: Value) -> Self {
        let content = raw.get("structuredContent").unwrap_or(&raw);
        let tree = raw
            .get("tree_markdown")
            .or_else(|| content.get("tree_markdown"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let snapshot_id = content
            .get("snapshot_id")
            .or_else(|| raw.get("snapshot_id"))
            .and_then(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| value.as_u64().map(|id| id.to_string()))
            });
        let elements = content
            .get("elements")
            .or_else(|| raw.get("elements"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Self {
            tree,
            snapshot_id,
            elements,
        }
    }

    pub fn element_by_index(&self, index: u64) -> Option<&Value> {
        self.elements
            .iter()
            .find(|element| element.get("element_index").and_then(Value::as_u64) == Some(index))
    }
}

pub fn element_index_of(tree: &str, needle: &str) -> Result<u64, String> {
    for line in tree.lines().filter(|line| line.contains(needle)) {
        if let Some(open) = line.find('[') {
            if let Some(close) = line[open + 1..].find(']') {
                if let Ok(index) = line[open + 1..open + 1 + close].parse() {
                    return Ok(index);
                }
            }
        }
    }
    Err(format!(
        "no indexed element matching {} in tree",
        json!(needle)
    ))
}

pub fn element_label(element: &Value) -> &str {
    element
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub fn element_role(element: &Value) -> &str {
    element
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub fn element_value(element: &Value) -> &str {
    element
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub fn element_frame(element: &Value) -> Option<Bounds> {
    let frame = element.get("frame")?;
    Some(Bounds {
        x: number(frame, "x"),
        y: number(frame, "y"),
        width: frame
            .get("w")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| number(frame, "width")),
        height: frame
            .get("h")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| number(frame, "height")),
    })
}

pub(crate) fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

pub(crate) fn window_area(window: &Value) -> f64 {
    window
        .get("bounds")
        .map(|bounds| number(bounds, "width") * number(bounds, "height"))
        .unwrap_or(0.0)
}

pub(crate) fn output_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        stderr.into_owned()
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

/// Runs `command` to its own exit and captures both streams; no deadline and
/// no poll (cli.md rule 8). `output()` drains both pipes while it waits.
pub(crate) fn run_to_exit(command: &mut Command) -> Result<Output, String> {
    command.output().map_err(|error| error.to_string())
}
