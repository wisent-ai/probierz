//! What the window has to show for a recorded run: the report with every
//! required entry, its recorded values, and nothing it must not show.

use super::super::*;

pub(crate) fn assert_report_visible(
    tree: &str,
    contract: &Value,
    recorded: &Recorded,
) -> Result<(), String> {
    if !tree.contains("id=conversation-entry-final") {
        return Err("Conversation must identify the durable final answer it rendered".to_string());
    }
    if tree.contains("id=conversation-entry-task_report") {
        return Err("Conversation must not render the task_report beside the final answer that already contains it".to_string());
    }
    let values = common::static_texts(tree);
    let expected = normalize(&recorded.final_text);
    if values
        .iter()
        .filter(|value| normalize(value) == expected)
        .count()
        != 1
    {
        return Err("Conversation must render the durable final answer exactly once".to_string());
    }
    let rendered_report = normalize(
        recorded
            .report
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let occurrences: usize = values
        .iter()
        .map(|value| normalize(value).matches(&rendered_report).count())
        .sum();
    if occurrences != 1 {
        return Err(
            "The real seven-point task report must appear exactly once in the native conversation"
                .to_string(),
        );
    }
    for requirement in contract
        .get("requirements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = requirement
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let title = requirement
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let entry = recorded
            .report
            .pointer(&format!("/report/{id}"))
            .unwrap_or(&Value::Null);
        let status = match entry.get("status").and_then(Value::as_str) {
            Some("not_applicable") => "not applicable",
            Some(status) => status,
            None => "",
        };
        let explanation = entry
            .get("explanation")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if !expected.contains(&normalize(&format!("{title} ({status}): {explanation}"))) {
            return Err(format!(
                "The native final answer must include the real {id} report explanation"
            ));
        }
        for evidence in entry
            .get("evidence")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !expected.contains(&normalize(evidence)) {
                return Err(format!(
                    "The native final answer must include the real {id} evidence reference"
                ));
            }
        }
    }
    Ok(())
}
