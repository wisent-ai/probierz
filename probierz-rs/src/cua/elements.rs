use serde_json::json;
use crate::cua::*;
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

pub(crate) fn tail_chars(text: &str, count: usize) -> String {
    let mut characters: Vec<char> = text.chars().rev().take(count).collect();
    characters.reverse();
    characters.into_iter().collect()
}

pub(crate) fn run_timeout(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let mut child: Child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|error| error.to_string()),
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => return Err(error.to_string()),
        }
    }
    let _ = child.kill();
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    Err(if output_detail(&output).trim().is_empty() {
        "command timed out".to_string()
    } else {
        output_detail(&output)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_prefers_structured_content_over_the_markdown_tree() {
        let snapshot = Snapshot::from_value(json!({
            "tree_markdown": "- AXButton (Save) [7]",
            "structuredContent": {
                "snapshot_id": "s42",
                "elements": [{"element_index": 7, "element_token": "s42:7", "label": "Save"}]
            }
        }));
        assert_eq!(snapshot.snapshot_id.as_deref(), Some("s42"));
        assert_eq!(element_index_of(&snapshot.tree, "AXButton (Save)"), Ok(7));
    }
}
